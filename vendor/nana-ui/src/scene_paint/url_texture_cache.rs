//! Shared URL texture lifecycle for quads and HostTexture masks.
//!
//! An image is sampled one of two ways ([`ImageSampling`]):
//!
//! - **Resample** (default): one level at the device-pixel size the image is
//!   painted at. Every fresh prepare notes what each target draws; the largest
//!   consumer wins, as for HostTexture `painted_extent`. The texture follows
//!   that demand with hysteresis — it grows at once, shrinks only after the
//!   smaller size held for [`SHRINK_DELAY`], ignores changes within
//!   `max(6px, 6%)` and demand under [`MIN_DEMAND`] (collapsing, animating).
//! - **Mipmap**: the decoded size plus a full mip chain, sampled trilinearly.
//!
//! Decoding, resampling and mip building happen on workers; the frame path
//! only uploads finished levels. A local image is still decoded on the frame
//! path the first time it is seen (unchanged from before) and shown at its
//! decoded size until the worker's resample or mip chain replaces it.
use std::{
    borrow::Cow,
    cell::Cell,
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

use nana_ui_core::{
    BackgroundImageFit, BackgroundPosition, BackgroundRepeat, ImageSampling, LengthSpec,
};
use nana_ui_platform::{FetchCancellation, SharedFetchHost};

use super::image_resample::{self, Level};
use super::image_url::{decode_bytes_rgba, decode_url_rgba, fetch_remote_bytes};
use crate::painted_demand::{Extents, PaintedDemand};

const MAX_FETCHES: usize = 4;
const RETAINED_BYTES: u64 = 64 * 1024 * 1024;
const RETAINED_ENTRIES: usize = 256;
const IDLE_FRAMES: u64 = 120;
/// How long a smaller painted size must hold before the texture shrinks to it.
const SHRINK_DELAY: Duration = Duration::from_secs(2);
/// A demand with an edge under this many device pixels is transitional
/// (collapsing, animating) and does not resize an already fitted texture.
const MIN_DEMAND: u32 = 16;
/// Appended to the URL to key its [`ImageSampling::Mipmap`] entry. NUL cannot
/// appear in a URL, a path or a generated key.
const MIPMAP_KEY: &str = "\u{0}mipmap";

pub(crate) type ImageWake = Arc<dyn Fn(&str) + Send + Sync>;

/// Which `FetchHost` a result was loaded through: the address of that host's
/// `Arc`. Results that did not touch the network — local files, `data:`,
/// generated keys, and remote URLs refused for want of a host — do not depend
/// on any host and share [`LOCAL`].
pub(crate) type Egress = usize;
const LOCAL: Egress = 0;

/// Identity of a host; `None` is [`LOCAL`].
pub(crate) fn egress_of(host: Option<&SharedFetchHost>) -> Egress {
    host.map_or(LOCAL, |host| Arc::as_ptr(host).cast::<()>() as usize)
}

fn is_remote(url: &str) -> bool {
    let url = url.trim();
    url.starts_with("http://") || url.starts_with("https://")
}

/// The cache key of `url` sampled as `sampling`: the URL itself for the
/// default, so bindings and retention keyed by URL keep working.
pub(crate) fn cache_key(url: &str, sampling: ImageSampling) -> Cow<'_, str> {
    match sampling {
        ImageSampling::Resample => Cow::Borrowed(url),
        ImageSampling::Mipmap => Cow::Owned(format!("{url}{MIPMAP_KEY}")),
    }
}

fn url_of(key: &str) -> &str {
    key.strip_suffix(MIPMAP_KEY).unwrap_or(key)
}

pub(crate) struct CachedUrlTexture {
    pub(crate) view: wgpu::TextureView,
    /// Size of level 0, which may be smaller than the image's natural size.
    pub(crate) width: u32,
    pub(crate) height: u32,
    #[cfg_attr(not(test), allow(dead_code, reason = "read by tests"))]
    pub(crate) mip_levels: u32,
    bytes: u64,
}

/// How many device pixels a use of an image needs.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Demand {
    /// Every texel (border-image slices, masks sampled in their own space).
    Full,
    /// A background or `<img>` layer; its drawn size depends on the natural
    /// size, which is not known before the image is decoded.
    Layer(LayerDemand),
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct LayerDemand {
    pub(crate) fit: BackgroundImageFit,
    pub(crate) size_width: Option<LengthSpec>,
    pub(crate) size_height: Option<LengthSpec>,
    pub(crate) position: BackgroundPosition,
    pub(crate) repeat: BackgroundRepeat,
    /// Layout box, logical px.
    pub(crate) box_size: [f32; 2],
    /// Device px per logical px along the image's own axes.
    pub(crate) scale: [f32; 2],
}

impl Demand {
    fn resolve(&self, natural: (u32, u32)) -> [u32; 2] {
        match self {
            Self::Full => [natural.0, natural.1],
            Self::Layer(layer) => {
                let [box_w, box_h] = layer.box_size;
                let dest = super::quad::url_dest_rect(
                    layer.fit,
                    layer.size_width,
                    layer.size_height,
                    layer.position,
                    layer.repeat,
                    box_w,
                    box_h,
                    natural.0 as f32,
                    natural.1 as f32,
                );
                [
                    device_pixels(dest[2] * box_w.max(1.0) * layer.scale[0]),
                    device_pixels(dest[3] * box_h.max(1.0) * layer.scale[1]),
                ]
            }
        }
    }
}

fn device_pixels(length: f32) -> u32 {
    if length.is_finite() && length > 0.0 {
        length.round().min(u32::MAX as f32) as u32
    } else {
        0
    }
}

/// Where a worker reads the pixels again.
#[derive(Clone)]
enum Source {
    /// Local file, `data:` or packaged: decoded again from the URL.
    Url(String),
    /// A remote image's fetched bytes, so a refit does not fetch again.
    Bytes { bytes: Arc<[u8]>, svg: bool },
}

impl Source {
    fn decode(&self) -> Option<Level> {
        let (width, height, rgba) = match self {
            Self::Url(url) => decode_url_rgba(url)?,
            Self::Bytes { bytes, svg } => decode_bytes_rgba(bytes, *svg)?,
        };
        Some(Level::new(width, height, rgba))
    }

    fn retained_bytes(&self) -> u64 {
        match self {
            Self::Url(_) => 0,
            Self::Bytes { bytes, .. } => bytes.len() as u64,
        }
    }
}

/// What a worker prepares.
#[derive(Debug, Clone, Copy)]
enum Goal {
    /// First load: fit the demand once the natural size is known.
    First(Demand),
    /// A refit to this level-0 size.
    Size((u32, u32)),
    Mipmap,
}

/// Levels ready for upload.
pub(crate) struct Prepared {
    natural: (u32, u32),
    levels: Vec<Level>,
}

impl Prepared {
    fn bytes(&self) -> u64 {
        self.levels
            .iter()
            .map(|level| level.rgba.len() as u64)
            .sum()
    }
}

fn prepare(level: Level, goal: Goal) -> Prepared {
    let natural = (level.width, level.height);
    let levels = match goal {
        Goal::First(demand) => {
            let [width, height] = demand.resolve(natural);
            let (width, height) = image_resample::fit_to_demand(
                natural,
                [width.max(MIN_DEMAND), height.max(MIN_DEMAND)],
            );
            vec![resampled(level, width, height)]
        }
        Goal::Size((width, height)) => vec![resampled(level, width, height)],
        Goal::Mipmap => image_resample::mip_chain(level),
    };
    Prepared { natural, levels }
}

fn resampled(level: Level, width: u32, height: u32) -> Level {
    if (level.width, level.height) == (width, height) {
        level
    } else {
        image_resample::resample(&level, width, height)
    }
}

/// A remote load: prepared levels, plus the bytes a later refit re-decodes.
pub(crate) struct Loaded {
    prepared: Prepared,
    source: Option<Source>,
}

type Decoded = Option<Loaded>;

fn load_remote(
    url: &str,
    host: &SharedFetchHost,
    cancellation: &FetchCancellation,
    goal: Goal,
) -> Decoded {
    let (bytes, svg) = fetch_remote_bytes(url, Some(host), cancellation)?;
    let (width, height, rgba) = decode_bytes_rgba(&bytes, svg)?;
    let prepared = prepare(Level::new(width, height, rgba), goal);
    let source = matches!(goal, Goal::First(_)).then(|| Source::Bytes {
        bytes: bytes.into(),
        svg,
    });
    Some(Loaded { prepared, source })
}

/// A resample or mip job on the prepare worker. Dropping the handle (the
/// entry was refit again, trimmed, or its bucket released) cancels it.
struct JobHandle {
    serial: u64,
    /// Level-0 size the job produces.
    size: (u32, u32),
    cancelled: Arc<AtomicBool>,
}

impl Drop for JobHandle {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
    }
}

struct Job {
    egress: Egress,
    key: String,
    serial: u64,
    source: Source,
    goal: Goal,
    cancelled: Arc<AtomicBool>,
    wake: Option<ImageWake>,
}

struct Done {
    egress: Egress,
    key: String,
    serial: u64,
    prepared: Option<Prepared>,
}

/// One thread per cache runs resample and mip jobs in order, skipping the
/// ones cancelled while queued. It exits when the cache drops its sender.
#[derive(Default)]
struct Worker {
    channel: Option<(mpsc::Sender<Job>, mpsc::Receiver<Done>)>,
    serial: u64,
}

impl Worker {
    fn submit(&mut self, mut job: Job, ready: &Arc<AtomicBool>) -> Option<JobHandle> {
        self.serial += 1;
        job.serial = self.serial;
        let handle = JobHandle {
            serial: self.serial,
            size: (0, 0),
            cancelled: Arc::clone(&job.cancelled),
        };
        if self.channel.is_none() {
            let (jobs, queue) = mpsc::channel::<Job>();
            let (done, results) = mpsc::channel();
            let ready = Arc::clone(ready);
            std::thread::Builder::new()
                .name("nana-image-prepare".into())
                .spawn(move || {
                    for job in queue {
                        if job.cancelled.load(Ordering::Acquire) {
                            continue;
                        }
                        let prepared = job.source.decode().map(|level| prepare(level, job.goal));
                        if job.cancelled.load(Ordering::Acquire) {
                            continue;
                        }
                        let key = job.key;
                        let wake = job.wake;
                        if done
                            .send(Done {
                                egress: job.egress,
                                key: key.clone(),
                                serial: job.serial,
                                prepared,
                            })
                            .is_err()
                        {
                            break;
                        }
                        ready.store(true, Ordering::Release);
                        if let Some(wake) = wake {
                            wake(url_of(&key));
                        }
                    }
                })
                .ok()?;
            self.channel = Some((jobs, results));
        }
        let (jobs, _) = self.channel.as_ref()?;
        jobs.send(job).ok()?;
        Some(handle)
    }

    fn drain(&self) -> Vec<Done> {
        self.channel
            .as_ref()
            .map(|(_, results)| results.try_iter().collect())
            .unwrap_or_default()
    }
}

struct Entry {
    texture: Option<CachedUrlTexture>,
    /// A finished CPU result. Only a later live lookup uploads it.
    ready: Option<Prepared>,
    /// What placement resolves against, whatever size the texture is.
    natural: (u32, u32),
    /// Where a refit reads pixels again; `None` never refits.
    source: Option<Source>,
    job: Option<JobHandle>,
    /// Sized from a painted demand at least once.
    fitted: bool,
    shrink: Option<PendingShrink>,
    used: Cell<u64>,
}

impl Entry {
    fn new(texture: Option<CachedUrlTexture>, natural: (u32, u32), used: u64) -> Self {
        Self {
            texture,
            ready: None,
            natural,
            source: None,
            job: None,
            fitted: false,
            shrink: None,
            used: Cell::new(used),
        }
    }

    fn bytes(&self) -> u64 {
        self.texture.as_ref().map_or(0, |texture| texture.bytes)
            + self.ready.as_ref().map_or(0, Prepared::bytes)
            + self.source.as_ref().map_or(0, Source::retained_bytes)
    }

    /// Level-0 size the entry shows or is about to show.
    fn current_size(&self) -> Option<(u32, u32)> {
        if let Some(job) = &self.job {
            return Some(job.size);
        }
        if let Some(level) = self.ready.as_ref().and_then(|ready| ready.levels.first()) {
            return Some((level.width, level.height));
        }
        self.texture
            .as_ref()
            .map(|texture| (texture.width, texture.height))
    }
}

/// An in-flight fetch, with the token that can stop it.
///
/// `used` is the same liveness stamp `Entry` carries: [`UrlTextureCache::load`]
/// refreshes it on every frame that still wants this URL, so a stale stamp means
/// nobody is waiting for the result any more.
struct Pending {
    receiver: mpsc::Receiver<Decoded>,
    cancellation: FetchCancellation,
    used: Cell<u64>,
}

#[cfg(test)]
impl Pending {
    fn for_test(receiver: mpsc::Receiver<Decoded>, used: u64) -> Self {
        Self {
            receiver,
            cancellation: FetchCancellation::new(),
            used: Cell::new(used),
        }
    }
}

/// Everything loaded through one egress.
#[derive(Default)]
struct Bucket {
    /// Keeps the address behind a remote bucket's [`Egress`] from being reused
    /// by another host while results loaded through this one are cached.
    /// Released with the bucket once it holds nothing.
    _host: Option<SharedFetchHost>,
    entries: HashMap<String, Entry>,
    pending: HashMap<String, Pending>,
}

/// Retains the current working set plus a bounded LRU of inactive textures.
/// HTTP work is limited to four concurrent requests per painter.
///
/// A painter owns one, shared by every pipeline that samples `url(...)`
/// images, so one URL is fetched, decoded and uploaded once however many
/// primitive kinds use it.
///
/// Remote results are partitioned by the fetch host they went through, so a
/// painter shared by documents with different policies never serves one
/// document an image only another document's policy admitted.
#[derive(Default)]
pub(crate) struct UrlTextureCache {
    buckets: HashMap<Egress, Bucket>,
    fetch_host: Option<SharedFetchHost>,
    ready: Arc<AtomicBool>,
    wake: Option<ImageWake>,
    frame: u64,
    deferred: bool,
    /// What the fresh prepare in progress draws, by key.
    pass: Extents,
    /// Merged demand of every target, by key.
    demand: PaintedDemand,
    /// Local entries decoded this pass and not yet fitted to a demand.
    unfitted: Vec<String>,
    /// Earliest pending shrink.
    next_shrink: Option<Instant>,
    worker: Worker,
}

impl UrlTextureCache {
    pub(crate) fn set_wake(&mut self, wake: ImageWake) {
        self.wake = Some(wake);
    }
    /// Egress for the remote URLs of the document painted next.
    pub(crate) fn set_fetch_host(&mut self, host: Option<SharedFetchHost>) {
        self.fetch_host = host;
    }
    fn egress(&self, url: &str) -> Egress {
        if is_remote(url) {
            egress_of(self.fetch_host.as_ref())
        } else {
            LOCAL
        }
    }
    /// Stop the in-flight requests started through `host` and drop what it
    /// loaded, releasing the bucket's hold on the host.
    pub(crate) fn release_fetch_host(&mut self, host: &SharedFetchHost) {
        if let Some(bucket) = self.buckets.remove(&egress_of(Some(host))) {
            for pending in bucket.pending.values() {
                pending.cancellation.cancel();
            }
        }
    }
    fn bucket_mut(&mut self, egress: Egress) -> &mut Bucket {
        let host = &self.fetch_host;
        self.buckets.entry(egress).or_insert_with(|| Bucket {
            _host: host.clone().filter(|_| egress != LOCAL),
            ..Bucket::default()
        })
    }
    fn pending_len(&self) -> usize {
        self.buckets
            .values()
            .map(|bucket| bucket.pending.len())
            .sum()
    }
    pub(crate) fn has_updates(&self) -> bool {
        self.ready.load(Ordering::Acquire)
    }
    /// A fetch, resample or mip job is still running.
    pub(crate) fn has_pending(&self) -> bool {
        self.pending_len() > 0
            || self.deferred
            || self
                .buckets
                .values()
                .flat_map(|bucket| bucket.entries.values())
                .any(|entry| entry.job.is_some())
    }
    /// A fresh prepare begins: a new frame stamp and an empty demand pass.
    pub(crate) fn begin_frame(&mut self) {
        self.frame = self.frame.wrapping_add(1);
        self.deferred = false;
        self.pass.clear();
    }
    pub(crate) fn get(&self, key: &str) -> Option<&Option<CachedUrlTexture>> {
        self.buckets
            .get(&self.egress(key))?
            .entries
            .get(key)
            .map(|entry| {
                entry.used.set(self.frame);
                &entry.texture
            })
    }
    pub(crate) fn contains_key(&self, key: &str) -> bool {
        self.get(key).is_some()
    }
    pub(crate) fn insert(&mut self, key: String, texture: Option<CachedUrlTexture>) {
        let natural = texture
            .as_ref()
            .map_or((0, 0), |texture| (texture.width, texture.height));
        let entry = Entry::new(texture, natural, self.frame);
        self.bucket_mut(self.egress(&key))
            .entries
            .insert(key, entry);
    }
    pub(crate) fn contains_retained(&self, key: &str) -> bool {
        self.buckets
            .get(&self.egress(key))
            .is_some_and(|bucket| bucket.entries.contains_key(key))
    }

    #[allow(dead_code)]
    pub(crate) fn load(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        url: &str,
    ) -> Option<(u32, u32)> {
        self.load_with_work(device, queue, url, None)
    }

    /// [`Self::load_image`] for a use that needs every texel of the image.
    pub(crate) fn load_with_work(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        url: &str,
        work: Option<&crate::gpu_work::GpuWorkSink>,
    ) -> Option<(u32, u32)> {
        self.load_image(
            device,
            queue,
            url,
            ImageSampling::Resample,
            Demand::Full,
            work,
        )
    }

    /// The natural size of `url` once a texture for it is uploaded, starting
    /// its load otherwise. `demand` is what this use draws; it counts toward
    /// the texture's size in [`ImageSampling::Resample`].
    pub(crate) fn load_image(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        url: &str,
        sampling: ImageSampling,
        demand: Demand,
        work: Option<&crate::gpu_work::GpuWorkSink>,
    ) -> Option<(u32, u32)> {
        let key = cache_key(url, sampling);
        let egress = self.egress(url);
        let frame = self.frame;
        if let Some(bucket) = self.buckets.get_mut(&egress) {
            if let Some(entry) = bucket.entries.get_mut(key.as_ref()) {
                if let Some(prepared) = entry.ready.take()
                    && let Some(texture) = upload_levels(device, queue, &prepared.levels, work)
                {
                    entry.texture = Some(texture);
                }
                entry.used.set(frame);
                let natural = entry.natural;
                entry.texture.as_ref()?;
                if sampling == ImageSampling::Resample {
                    self.note(&key, demand.resolve(natural));
                }
                return Some(natural);
            }
            if let Some(pending) = bucket.pending.get(key.as_ref()) {
                pending.used.set(frame);
                return None;
            }
        }
        let goal = match sampling {
            ImageSampling::Resample => Goal::First(demand),
            ImageSampling::Mipmap => Goal::Mipmap,
        };
        if is_remote(url) {
            // No host means no egress: refused here, before any socket.
            let Some(host) = self.fetch_host.clone() else {
                self.insert(key.into_owned(), None);
                return None;
            };
            if self.pending_len() >= MAX_FETCHES {
                self.deferred = true;
                return None;
            }
            let (tx, rx) = mpsc::channel();
            let worker_url = url.to_owned();
            let ready = self.ready.clone();
            let wake = self.wake.clone();
            let cancellation = FetchCancellation::new();
            let worker_cancellation = cancellation.clone();
            // Workers own only CPU bytes. Texture creation/upload stays on the
            // host. The worker captures the requesting document's host, so a
            // later paint with another host cannot redirect this request.
            let spawned = std::thread::Builder::new()
                .name("nana-image".into())
                .spawn(move || {
                    let decoded = load_remote(&worker_url, &host, &worker_cancellation, goal);
                    if tx.send(decoded).is_ok() {
                        ready.store(true, Ordering::Release);
                        if let Some(wake) = wake {
                            wake(&worker_url);
                        }
                    }
                });
            if spawned.is_ok() {
                self.bucket_mut(egress).pending.insert(
                    key.into_owned(),
                    Pending {
                        receiver: rx,
                        cancellation,
                        used: Cell::new(frame),
                    },
                );
            } else {
                self.insert(key.into_owned(), None);
            }
            return None;
        }
        let Some((width, height, rgba)) = decode_url_rgba(url) else {
            self.insert(key.into_owned(), None);
            return None;
        };
        let Some(texture) = upload_mips(device, queue, &[(width, height, &rgba)], work) else {
            self.insert(key.into_owned(), None);
            return None;
        };
        // Resized or mip-chained on the worker, which decodes the URL again.
        let natural = (width, height);
        let source = Source::Url(url.to_owned());
        let mut entry = Entry::new(Some(texture), natural, frame);
        match sampling {
            ImageSampling::Resample => {
                entry.source = Some(source);
                self.note(&key, demand.resolve(natural));
                self.unfitted.push(key.to_string());
            }
            ImageSampling::Mipmap => {
                entry.job = self.submit(egress, &key, source, goal, natural);
            }
        }
        self.bucket_mut(egress)
            .entries
            .insert(key.into_owned(), entry);
        Some(natural)
    }

    fn note(&mut self, key: &str, extent: [u32; 2]) {
        self.pass
            .entry(Arc::from(key))
            .and_modify(|current| {
                *current = [current[0].max(extent[0]), current[1].max(extent[1])];
            })
            .or_insert(extent);
    }

    fn submit(
        &mut self,
        egress: Egress,
        key: &str,
        source: Source,
        goal: Goal,
        size: (u32, u32),
    ) -> Option<JobHandle> {
        let job = Job {
            egress,
            key: key.to_owned(),
            serial: 0,
            source,
            goal,
            cancelled: Arc::default(),
            wake: self.wake.clone(),
        };
        let mut handle = self.worker.submit(job, &self.ready)?;
        handle.size = size;
        Some(handle)
    }

    /// A fresh prepare of target `channel` ended: its pass replaces what it
    /// drew before, and every image whose merged demand moved is refit.
    pub(crate) fn commit_demand(&mut self, channel: u64) {
        let mut changed = self.demand.commit(channel, &mut self.pass);
        changed.extend(self.unfitted.drain(..).map(Arc::from));
        let now = Instant::now();
        for key in changed {
            self.refit(&key, now);
        }
    }

    /// Target `channel` is gone or shows nothing: withdraw its demand.
    pub(crate) fn retire_demand(&mut self, channel: u64) {
        let now = Instant::now();
        for key in self.demand.retire(channel) {
            self.refit(&key, now);
        }
    }

    fn refit(&mut self, key: &str, now: Instant) {
        let demand = self.demand.extent(key);
        let egress = self.egress(key);
        let Some(entry) = self
            .buckets
            .get_mut(&egress)
            .and_then(|bucket| bucket.entries.get_mut(key))
        else {
            return;
        };
        let (Some(source), Some(current)) = (entry.source.clone(), entry.current_size()) else {
            return;
        };
        let plan = if entry.fitted {
            plan_refit(current, entry.natural, demand, &mut entry.shrink, now)
        } else {
            first_fit(current, entry.natural, demand)
        };
        entry.fitted |= demand.is_some();
        match plan {
            Refit::Keep => {}
            Refit::At(due) => {
                self.next_shrink = Some(self.next_shrink.map_or(due, |next| next.min(due)));
            }
            Refit::Now(size) => {
                // Replacing the handle cancels a job still queued for the old size.
                let job = self.submit(egress, key, source, Goal::Size(size), size);
                if let Some(entry) = self
                    .buckets
                    .get_mut(&egress)
                    .and_then(|bucket| bucket.entries.get_mut(key))
                {
                    entry.job = job;
                }
            }
        }
    }

    /// Collect CPU results. Only a subsequent live URL lookup may upload them.
    pub(crate) fn poll(&mut self) -> bool {
        self.ready.store(false, Ordering::Release);
        let used = self.frame.saturating_sub(1);
        let mut changed = false;
        for bucket in self.buckets.values_mut() {
            let mut complete = Vec::new();
            bucket
                .pending
                .retain(|key, pending| match pending.receiver.try_recv() {
                    Ok(decoded) => {
                        complete.push((key.clone(), decoded));
                        false
                    }
                    Err(mpsc::TryRecvError::Disconnected) => {
                        complete.push((key.clone(), None));
                        false
                    }
                    Err(mpsc::TryRecvError::Empty) => true,
                });
            changed |= !complete.is_empty();
            for (key, decoded) in complete {
                let entry = match decoded {
                    Some(Loaded { prepared, source }) => {
                        let mut entry = Entry::new(None, prepared.natural, used);
                        entry.ready = Some(prepared);
                        entry.source = source;
                        entry.fitted = true;
                        entry
                    }
                    None => Entry::new(None, (0, 0), used),
                };
                bucket.entries.insert(key, entry);
            }
        }
        for done in self.worker.drain() {
            let Some(entry) = self
                .buckets
                .get_mut(&done.egress)
                .and_then(|bucket| bucket.entries.get_mut(&done.key))
            else {
                continue;
            };
            if entry
                .job
                .as_ref()
                .is_none_or(|job| job.serial != done.serial)
            {
                continue;
            }
            entry.job = None;
            match done.prepared {
                Some(prepared) => {
                    entry.ready = Some(prepared);
                    changed = true;
                }
                // The source went away (file removed): keep what is shown.
                None => entry.source = None,
            }
        }
        let now = Instant::now();
        if self.next_shrink.is_some_and(|due| now >= due) {
            self.next_shrink = None;
            let shrinking: Vec<String> = self
                .buckets
                .values()
                .flat_map(|bucket| bucket.entries.iter())
                .filter(|(_, entry)| entry.shrink.is_some())
                .map(|(key, _)| key.clone())
                .collect();
            for key in shrinking {
                self.refit(&key, now);
            }
        }
        changed
    }

    /// Drop in-flight fetches nobody has asked for in `IDLE_FRAMES`.
    ///
    /// The threshold is deliberately generous: an image scrolled out of view for
    /// a few frames is cheaper to let finish than to cancel and re-request when
    /// it scrolls back. Two seconds of nobody asking means it was abandoned.
    ///
    /// Cancelled entries are removed outright rather than left for [`Self::poll`]
    /// to reap, because `poll` records a dead receiver as `None` — a permanent
    /// "this URL failed" in `entries`. A cancellation is not a failure: dropping
    /// the entry returns the URL to "never loaded", so a node that comes back
    /// re-requests it.
    fn cancel_unreferenced_fetches(&mut self) {
        let frame = self.frame;
        for bucket in self.buckets.values_mut() {
            bucket.pending.retain(|_, pending| {
                if frame.saturating_sub(pending.used.get()) <= IDLE_FRAMES {
                    return true;
                }
                pending.cancellation.cancel();
                false
            });
        }
    }

    pub(crate) fn trim(&mut self) {
        self.cancel_unreferenced_fetches();
        let mut inactive = Vec::new();
        let mut bytes = 0;
        for (egress, bucket) in &self.buckets {
            for (key, entry) in &bucket.entries {
                if entry.used.get() == self.frame {
                    continue;
                }
                let size = entry.bytes();
                bytes += size;
                inactive.push((entry.used.get(), *egress, key.clone(), size));
            }
        }
        inactive.sort_unstable_by_key(|entry| entry.0);
        let mut count = inactive.len();
        for (used, egress, key, size) in inactive {
            if count <= RETAINED_ENTRIES
                && bytes <= RETAINED_BYTES
                && self.frame.saturating_sub(used) <= IDLE_FRAMES
            {
                break;
            }
            if let Some(bucket) = self.buckets.get_mut(&egress) {
                bucket.entries.remove(&key);
            }
            count -= 1;
            bytes -= size;
        }
        // An emptied bucket releases its host: nothing loaded through it is
        // left to protect from a reused address.
        self.buckets
            .retain(|_, bucket| !bucket.entries.is_empty() || !bucket.pending.is_empty());
    }
}

/// Stop in-flight fetches when the pipeline goes away.
///
/// The worker threads are detached, so without this a closed window leaves them
/// parked on a socket until the policy timeout with nobody left to receive the
/// result. Cancelling shuts the socket down; the worker's `send` then fails
/// against the dropped receiver and it exits.
///
/// This ends the network wait, not an in-progress decode — `image` and `resvg`
/// have no cancellation point, so a worker already decoding runs to completion.
/// Queued resample jobs are cancelled with their entries.
impl Drop for UrlTextureCache {
    fn drop(&mut self) {
        for bucket in self.buckets.values() {
            for pending in bucket.pending.values() {
                pending.cancellation.cancel();
            }
        }
    }
}

/// A smaller target waiting to hold for [`SHRINK_DELAY`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PendingShrink {
    target: (u32, u32),
    since: Instant,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Refit {
    /// The texture fits, or the demand is transitional.
    Keep,
    /// Prepare this level-0 size now.
    Now((u32, u32)),
    /// A shrink, not due before this instant.
    At(Instant),
}

/// Changes within `max(6px, 6%)` of the larger size are not worth a resample.
fn resize_needed(current: (u32, u32), target: (u32, u32)) -> bool {
    let largest = current.0.max(current.1).max(target.0).max(target.1);
    let slack = ((largest as f32 * 0.06) as u32).max(6);
    current.0.abs_diff(target.0) > slack || current.1.abs_diff(target.1) > slack
}

/// The first decision for a texture shown at its decoded size: fit the
/// demand now, reading a tiny demand as [`MIN_DEMAND`].
fn first_fit(current: (u32, u32), natural: (u32, u32), demand: Option<[u32; 2]>) -> Refit {
    let Some([width, height]) = demand else {
        return Refit::Keep;
    };
    let target =
        image_resample::fit_to_demand(natural, [width.max(MIN_DEMAND), height.max(MIN_DEMAND)]);
    if resize_needed(current, target) {
        Refit::Now(target)
    } else {
        Refit::Keep
    }
}

/// How big a fitted texture should be for the merged `demand`.
///
/// Grow at once; shrink only once the smaller target held for
/// [`SHRINK_DELAY`], restarting the wait whenever the target moves. No demand
/// (not drawn) or an edge under [`MIN_DEMAND`] is transitional: keep.
fn plan_refit(
    current: (u32, u32),
    natural: (u32, u32),
    demand: Option<[u32; 2]>,
    shrink: &mut Option<PendingShrink>,
    now: Instant,
) -> Refit {
    let Some(demand) =
        demand.filter(|[width, height]| *width >= MIN_DEMAND && *height >= MIN_DEMAND)
    else {
        *shrink = None;
        return Refit::Keep;
    };
    let target = image_resample::fit_to_demand(natural, demand);
    if !resize_needed(current, target) {
        *shrink = None;
        return Refit::Keep;
    }
    if target.0 > current.0 || target.1 > current.1 {
        *shrink = None;
        return Refit::Now(target);
    }
    let since = match *shrink {
        Some(pending) if !resize_needed(pending.target, target) => pending.since,
        _ => now,
    };
    let due = since + SHRINK_DELAY;
    if now >= due {
        *shrink = None;
        return Refit::Now(target);
    }
    *shrink = Some(PendingShrink { target, since });
    Refit::At(due)
}

#[cfg(test)]
thread_local! {
    /// URL textures uploaded on this thread, whichever cache asked.
    pub(crate) static UPLOADS: Cell<usize> = const { Cell::new(0) };
}

pub(crate) fn upload_with_work(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    (width, height, rgba): (u32, u32, &[u8]),
    work: Option<&crate::gpu_work::GpuWorkSink>,
) -> Option<CachedUrlTexture> {
    upload_mips(device, queue, &[(width, height, rgba)], work)
}

fn upload_levels(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    levels: &[Level],
    work: Option<&crate::gpu_work::GpuWorkSink>,
) -> Option<CachedUrlTexture> {
    let levels: Vec<_> = levels
        .iter()
        .map(|level| (level.width, level.height, level.rgba.as_slice()))
        .collect();
    upload_mips(device, queue, &levels, work)
}

/// One texture whose mip `i` is `levels[i]`; each level halves the one above.
fn upload_mips(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    levels: &[(u32, u32, &[u8])],
    work: Option<&crate::gpu_work::GpuWorkSink>,
) -> Option<CachedUrlTexture> {
    let &(width, height, _) = levels.first()?;
    let limit = device.limits().max_texture_dimension_2d;
    if width == 0 || height == 0 || width > limit || height > limit {
        return None;
    }
    #[cfg(test)]
    UPLOADS.with(|uploads| uploads.set(uploads.get() + 1));
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("nana-ui.scene.url"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: levels.len() as u32,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let mut bytes = 0;
    for (mip_level, &(width, height, rgba)) in levels.iter().enumerate() {
        let mip_level = mip_level as u32;
        bytes += rgba.len() as u64;
        if let Some(work) = work {
            work.write_texture_level(
                queue,
                &texture,
                mip_level,
                rgba,
                4 * width,
                height,
                [width, height, 1],
            );
        } else {
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                rgba,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(4 * width),
                    rows_per_image: Some(height),
                },
                wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
            );
        }
    }
    Some(CachedUrlTexture {
        view: texture.create_view(&Default::default()),
        width,
        height,
        mip_levels: levels.len() as u32,
        bytes,
    })
}

#[cfg(test)]
impl UrlTextureCache {
    /// Level-0 size and mip count of the texture uploaded for `key`.
    pub(crate) fn texture_shape(&self, key: &str) -> Option<(u32, u32, u32)> {
        self.buckets
            .get(&self.egress(key))?
            .entries
            .get(key)?
            .texture
            .as_ref()
            .map(|texture| (texture.width, texture.height, texture.mip_levels))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    impl UrlTextureCache {
        fn entry_keys(&self) -> Vec<&String> {
            self.buckets
                .values()
                .flat_map(|bucket| bucket.entries.keys())
                .collect()
        }
        fn entries(&self) -> impl Iterator<Item = &Entry> {
            self.buckets
                .values()
                .flat_map(|bucket| bucket.entries.values())
        }
        fn insert_pending(&mut self, url: &str, pending: Pending) {
            let egress = self.egress(url);
            self.bucket_mut(egress).pending.insert(url.into(), pending);
        }
        fn pending(&self, url: &str) -> Option<&Pending> {
            self.buckets.get(&self.egress(url))?.pending.get(url)
        }
    }

    #[test]
    fn inactive_entries_are_bounded_and_eventually_released() {
        let mut cache = UrlTextureCache::default();
        cache.begin_frame();
        for id in 0..1000 {
            cache.insert(format!("failed:{id}"), None);
        }
        cache.trim();
        cache.begin_frame();
        cache.trim();
        assert!(cache.entry_keys().len() <= RETAINED_ENTRIES);
        let kept = cache.entry_keys()[0].clone();
        for _ in 0..=IDLE_FRAMES {
            cache.begin_frame();
            cache.get(&kept);
            cache.trim();
        }
        assert_eq!(
            cache.entry_keys().len(),
            1,
            "only the referenced image may survive expiry"
        );
        assert!(cache.contains_retained(&kept));
    }

    #[test]
    fn completed_unused_images_stay_on_cpu_and_obey_residency_budget() {
        let mut cache = UrlTextureCache::default();
        cache.begin_frame();
        for id in 0..5 {
            let (sender, receiver) = mpsc::channel();
            let used = cache.frame;
            cache.insert_pending(&format!("old:{id}"), Pending::for_test(receiver, used));
            sender
                .send(Some(Loaded {
                    prepared: Prepared {
                        natural: (2048, 2048),
                        levels: vec![Level::new(2048, 2048, vec![255; 2048 * 2048 * 4])],
                    },
                    source: None,
                }))
                .unwrap();
        }
        cache.begin_frame();
        assert!(cache.poll());
        assert!(cache.entries().all(|entry| entry.texture.is_none()));
        cache.trim();
        let bytes: u64 = cache.entries().map(Entry::bytes).sum();
        assert!(bytes <= RETAINED_BYTES);
    }

    #[test]
    fn dropping_the_cache_cancels_in_flight_fetches() {
        let (_sender, receiver) = mpsc::channel();
        let mut cache = UrlTextureCache::default();
        cache.begin_frame();
        let pending = Pending::for_test(receiver, cache.frame);
        // The clone outlives the cache, which is what makes the assertion
        // observable after the drop.
        let token = pending.cancellation.clone();
        cache.insert_pending("http://example.invalid/a.png", pending);

        assert!(
            !token.is_cancelled(),
            "not cancelled while the cache is alive"
        );
        drop(cache);
        assert!(
            token.is_cancelled(),
            "tearing the pipeline down must stop the in-flight request"
        );
    }

    #[test]
    fn unreferenced_pending_fetches_are_cancelled_and_stay_retryable() {
        let url = "http://example.invalid/b.png";
        let (_sender, receiver) = mpsc::channel();
        let mut cache = UrlTextureCache::default();
        cache.begin_frame();
        let pending = Pending::for_test(receiver, cache.frame);
        let token = pending.cancellation.clone();
        cache.insert_pending(url, pending);

        for _ in 0..=IDLE_FRAMES {
            cache.begin_frame();
            cache.trim();
        }

        assert!(token.is_cancelled(), "an abandoned fetch must be cancelled");
        assert!(!cache.has_pending(), "and must stop occupying a slot");
        assert!(
            !cache.contains_retained(url),
            "cancelling is not failing: the URL must stay re-requestable, not be              cached as a permanent miss"
        );
    }

    #[test]
    fn a_pending_fetch_still_wanted_each_frame_is_not_cancelled() {
        let url = "http://example.invalid/c.png";
        let (_sender, receiver) = mpsc::channel();
        let mut cache = UrlTextureCache::default();
        cache.begin_frame();
        let pending = Pending::for_test(receiver, cache.frame);
        let token = pending.cancellation.clone();
        cache.insert_pending(url, pending);

        for _ in 0..IDLE_FRAMES * 3 {
            cache.begin_frame();
            // What `load` does for a URL that is still painted this frame.
            cache.pending(url).unwrap().used.set(cache.frame);
            cache.trim();
        }

        assert!(
            !token.is_cancelled(),
            "a live request must not be cancelled"
        );
        assert_eq!(cache.pending_len(), 1);
    }

    #[test]
    fn an_idle_remote_bucket_releases_its_fetch_host() {
        let host = nana_ui_platform::shared_fetch_host(nana_ui_platform::NativeFetchHost::new(
            nana_ui_platform::FetchPolicy::default(),
        ));
        let released = Arc::downgrade(&host);
        let mut cache = UrlTextureCache::default();
        cache.begin_frame();
        cache.set_fetch_host(Some(host));
        cache.insert("http://example.invalid/e.png".into(), None);
        cache.set_fetch_host(None);
        assert!(
            released.upgrade().is_some(),
            "cached results pin their host"
        );

        for _ in 0..=IDLE_FRAMES {
            cache.begin_frame();
            cache.trim();
        }

        assert!(
            released.upgrade().is_none(),
            "a closed document's host must not outlive its expired images"
        );
    }

    #[test]
    fn mipmap_entries_are_keyed_apart_from_the_url() {
        let url = "http://example.invalid/f.png";
        assert_eq!(cache_key(url, ImageSampling::Resample), url);
        let mip = cache_key(url, ImageSampling::Mipmap);
        assert_ne!(mip, url);
        assert_eq!(url_of(&mip), url);
        assert!(is_remote(&mip), "the key keeps the URL's egress");
    }

    #[test]
    fn a_first_fit_sizes_now_and_reads_a_tiny_demand_as_the_minimum() {
        assert_eq!(first_fit((1920, 1080), (1920, 1080), None), Refit::Keep);
        assert_eq!(
            first_fit((1920, 1080), (1920, 1080), Some([320, 180])),
            Refit::Now((320, 180))
        );
        assert_eq!(
            first_fit((640, 640), (640, 640), Some([4, 4])),
            Refit::Now((16, 16))
        );
        assert_eq!(
            first_fit((64, 64), (64, 64), Some([400, 400])),
            Refit::Keep,
            "never larger than the source"
        );
    }

    #[test]
    fn refits_grow_at_once_and_shrink_only_after_the_delay() {
        let natural = (1920, 1080);
        let start = Instant::now();
        let mut shrink = None;
        assert_eq!(
            plan_refit((320, 180), natural, Some([640, 360]), &mut shrink, start),
            Refit::Now((640, 360)),
            "growing is immediate"
        );
        assert_eq!(
            plan_refit((640, 360), natural, Some([620, 349]), &mut shrink, start),
            Refit::Keep,
            "a change within the slack is ignored"
        );
        assert_eq!(
            plan_refit((640, 360), natural, Some([8, 8]), &mut shrink, start),
            Refit::Keep,
            "a collapsing demand is transitional"
        );
        assert_eq!(
            plan_refit((640, 360), natural, None, &mut shrink, start),
            Refit::Keep,
            "an undrawn image keeps its texture"
        );
        let due = start + SHRINK_DELAY;
        assert_eq!(
            plan_refit((640, 360), natural, Some([320, 180]), &mut shrink, start),
            Refit::At(due)
        );
        let later = start + SHRINK_DELAY / 2;
        assert_eq!(
            plan_refit((640, 360), natural, Some([322, 181]), &mut shrink, later),
            Refit::At(due),
            "a target within the slack keeps the timer"
        );
        assert_eq!(
            plan_refit((640, 360), natural, Some([160, 90]), &mut shrink, later),
            Refit::At(later + SHRINK_DELAY),
            "a new smaller target restarts the timer"
        );
        assert_eq!(
            plan_refit(
                (640, 360),
                natural,
                Some([160, 90]),
                &mut shrink,
                later + SHRINK_DELAY
            ),
            Refit::Now((160, 90))
        );
        assert_eq!(shrink, None);
    }

    #[test]
    fn a_layer_demand_resolves_its_drawn_size_against_the_natural_size() {
        let layer = |fit| {
            Demand::Layer(LayerDemand {
                fit,
                size_width: None,
                size_height: None,
                position: BackgroundPosition::center(),
                repeat: BackgroundRepeat::NoRepeat,
                box_size: [100.0, 100.0],
                scale: [2.0, 2.0],
            })
        };
        assert_eq!(
            layer(BackgroundImageFit::Contain).resolve((400, 200)),
            [200, 100]
        );
        assert_eq!(
            layer(BackgroundImageFit::Cover).resolve((400, 200)),
            [400, 200]
        );
        assert_eq!(
            layer(BackgroundImageFit::Stretch).resolve((400, 200)),
            [200, 200]
        );
        assert_eq!(Demand::Full.resolve((400, 200)), [400, 200]);
    }
}
