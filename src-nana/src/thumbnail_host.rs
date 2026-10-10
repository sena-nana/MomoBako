//! 把解码后的缩略图像素和文件预览的画面登记成宿主纹理。
//!
//! 缩略图像素不放进会整份克隆的壳层状态，布局只用原始宽高，绘制用缩小后的纹理。纹理只在上传时登记一次，
//! 句柄留在账本里（`thumbnail_cache.rs`）：同一个句柄再登记不会变，换一个新句柄就算换了纹理，宿主纹理表
//! 会叫窗口重画，所以空闲帧不能再登记。回收按挂着的缩略图和当前列表来：在用的不卸，换了目录的立刻卸，
//! 滚出视口的超过上限时按最近最少使用卸；卸掉的行回到类型图标，再挂出来时重新解码。
//!
//! 文件预览只在预览框这一帧从 `file-preview` 槽取画面时上传，也是上传时登记一次；播放列表幻灯片、PDF 和
//! 模型页图都编成内容图直接画，不经宿主纹理。

use nana_ui::{
    ApplicationWindow, GpuTextureDescriptor, GpuTextureFormat, GpuTextureRegion, GpuTextureUsages, HostTexture,
    HostTextureAlphaMode, HostTextureRegistry, RuntimeProgramContext,
};

use crate::shell::{thumbnail_slot, ShellMessage, ShellViewModel, ThumbnailFrame};
use crate::thumbnail_cache::{ThumbnailCache, THUMBNAIL_CAPACITY};
use crate::{MomoBakoApplication, NativePreviewGpu};

/// 等下一帧上传的缩略图像素。
pub(crate) struct PendingThumb {
    pub(crate) path: String,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) rgba: Vec<u8>,
}

/// 宿主纹理表：登记和移除槽。产品里是窗口的 `HostTextureRegistry`，测试里记下每次调用。
pub(crate) trait TextureTable<T> {
    fn register(&mut self, slot: &str, texture: &T, width: u32, height: u32);
    fn remove(&mut self, slot: &str);
}

impl TextureTable<HostTexture> for HostTextureRegistry {
    fn register(&mut self, slot: &str, texture: &HostTexture, width: u32, height: u32) {
        HostTextureRegistry::register(self, slot, texture.clone(), width, height, HostTextureAlphaMode::Premultiplied);
    }

    fn remove(&mut self, slot: &str) {
        HostTextureRegistry::remove(self, slot);
    }
}

/// 缩略图纹理的宿主端：等上传的像素和已经登记的纹理。
pub(crate) struct ThumbnailHost<T> {
    pending: Vec<PendingThumb>,
    cache: ThumbnailCache<T>,
    capacity: usize,
    /// 上一次回收时的壳层版本和挂着的缩略图版本。都没变、也没有新像素时这一帧什么也不做。
    swept: Option<(u64, u64)>,
}

impl<T> Default for ThumbnailHost<T> {
    fn default() -> Self {
        Self::with_capacity(THUMBNAIL_CAPACITY)
    }
}

impl<T> ThumbnailHost<T> {
    /// 缓存上限为 `capacity` 张的宿主端。产品用 [`THUMBNAIL_CAPACITY`]。
    pub(crate) fn with_capacity(capacity: usize) -> Self {
        Self { pending: Vec::new(), cache: ThumbnailCache::default(), capacity, swept: None }
    }

    /// 从解码结果里拿走像素，留给下一帧上传。宽高仍留在消息里给布局用。
    pub(crate) fn queue(&mut self, frames: &mut [ThumbnailFrame]) {
        for frame in frames {
            let rgba = std::mem::take(&mut frame.rgba);
            if rgba.len() != (frame.width as usize).saturating_mul(frame.height as usize).saturating_mul(4) {
                eprintln!("Nana 缩略图像素长度不对：{}", frame.path);
                continue;
            }
            self.pending.push(PendingThumb { path: frame.path.clone(), width: frame.width, height: frame.height, rgba });
        }
    }

    /// 已经登记纹理的缩略图张数。
    #[cfg(test)]
    pub(crate) fn registered(&self) -> usize {
        self.cache.len()
    }

    /// 准备帧：上传新到的缩略图并登记，再按挂着的缩略图和当前列表回收纹理。
    ///
    /// 算法：解码回来时已经不在列表里、也不挂着的像素不上传；上传失败的告诉文件表面，行回到类型图标、
    /// 不再重试。没有新像素、壳层版本和挂着的缩略图版本都没变时到此为止，不登记也不移除，宿主纹理表不会叫
    /// 窗口重画。否则回收一轮（规则见 `thumbnail_cache.rs`），卸掉的从纹理表移除，对应的行回到类型图标；
    /// 行变了就标脏，下一次同步换掉卡片的样子（这些行都不挂着，画面上看不到变化）。宿主还没报告过挂着的
    /// 缩略图（没有视图）时不回收。
    pub(crate) fn publish(
        &mut self,
        shell: &mut ShellViewModel,
        table: &mut impl TextureTable<T>,
        mut upload: impl FnMut(&PendingThumb) -> Result<T, String>,
    ) {
        let pending = std::mem::take(&mut self.pending);
        let uploaded = !pending.is_empty();
        if uploaded {
            let listed = shell.files.listed_thumbnails();
            let mut failed = Vec::new();
            for thumb in pending {
                let mounted = shell.files.mounted_thumbnails().is_some_and(|mounted| mounted.contains(&thumb.path));
                if !mounted && !listed.contains(&thumb.path) {
                    continue;
                }
                match upload(&thumb) {
                    Ok(texture) => {
                        table.register(&thumbnail_slot(&thumb.path), &texture, thumb.width, thumb.height);
                        self.cache.insert(thumb.path, texture);
                    }
                    Err(error) => {
                        eprintln!("Nana 缩略图纹理上传失败：{}：{error}", thumb.path);
                        failed.push(thumb.path);
                    }
                }
            }
            if shell.files.note_thumbnails_failed(&failed) {
                shell.mark_surface_dirty();
            }
        }
        let key = (shell.revision, shell.files.mounted_version());
        if !uploaded && self.swept == Some(key) {
            return;
        }
        let evicted = match shell.files.mounted_thumbnails() {
            Some(in_use) => self.cache.sweep(in_use, &shell.files.listed_thumbnails(), self.capacity),
            None => Vec::new(),
        };
        for path in &evicted {
            table.remove(&thumbnail_slot(path));
        }
        if shell.files.note_thumbnails_released(&evicted) {
            shell.mark_surface_dirty();
        }
        self.swept = Some((shell.revision, shell.files.mounted_version()));
    }
}

impl MomoBakoApplication {
    /// 解码结果到了：像素留给下一次准备帧上传。
    pub(crate) fn queue_thumbnail_frames(&mut self, frames: &mut [ThumbnailFrame]) {
        self.thumbnails.queue(frames);
    }

    /// 准备帧：上传新缩略图、回收不用的纹理。空闲帧不登记，不叫窗口重画。
    pub(crate) fn publish_thumbnails(&mut self, window: &mut ApplicationWindow, context: &RuntimeProgramContext<ShellMessage>) {
        self.thumbnails.publish(&mut self.shell, &mut window.textures, |thumb| upload_texture(context, thumb));
    }
}

/// 建一张可采样的纹理并写入像素。
fn upload_texture(context: &RuntimeProgramContext<ShellMessage>, thumb: &PendingThumb) -> Result<HostTexture, String> {
    let texture = context
        .gpu()
        .create_texture(&GpuTextureDescriptor {
            label: Some("momobako thumbnail"),
            width: thumb.width,
            height: thumb.height,
            format: GpuTextureFormat::RGBA8_UNORM_SRGB,
            usage: GpuTextureUsages::SAMPLED | GpuTextureUsages::COPY_DST,
        })
        .map_err(|error| format!("纹理创建失败（{}x{}）：{error}", thumb.width, thumb.height))?;
    context
        .gpu()
        .write_texture(&texture, GpuTextureRegion::full(thumb.width, thumb.height), &thumb.rgba, thumb.width.saturating_mul(4))
        .map_err(|error| format!("像素写入失败：{error}"))?;
    Ok(HostTexture::new(1, 1, &texture))
}

/// 文件预览的图片或视频画面。预览框这一帧不从 `file-preview` 槽取画面（预览页藏着、预览的是幻灯片
/// 当前项、PDF 和模型页图），或者没有令牌和像素时不上传，已经上传的卸掉。画面换了（令牌或尺寸不同）才
/// 上传并登记一次，同一张画面不再登记。
pub(crate) fn publish_preview(
    app: &mut MomoBakoApplication,
    window: &mut ApplicationWindow,
    context: &RuntimeProgramContext<ShellMessage>,
) {
    let wanted = crate::shell::wants_preview_texture(&app.shell);
    let (true, Some(token), Some(pixels)) = (wanted, app.shell.preview_token.clone(), app.shell.preview_pixels.as_ref()) else {
        app.preview_gpu = None;
        window.textures.remove("file-preview");
        return;
    };
    let current = app.preview_gpu.as_ref().is_some_and(|preview| {
        preview.token == token && preview.width == pixels.width && preview.height == pixels.height
    });
    if current {
        return;
    }
    let Ok(texture) = context.gpu().create_texture(&GpuTextureDescriptor {
        label: Some("momobako file preview"),
        width: pixels.width,
        height: pixels.height,
        format: GpuTextureFormat::RGBA8_UNORM_SRGB,
        usage: GpuTextureUsages::SAMPLED | GpuTextureUsages::COPY_DST,
    }) else {
        eprintln!("Nana 文件预览纹理创建失败：{}x{}", pixels.width, pixels.height);
        return;
    };
    if let Err(error) = context.gpu().write_texture(
        &texture,
        GpuTextureRegion::full(pixels.width, pixels.height),
        &pixels.rgba,
        pixels.width.saturating_mul(4),
    ) {
        eprintln!("Nana 文件预览纹理上传失败：{error}");
        return;
    }
    window.textures.register(
        "file-preview",
        HostTexture::new(1, 1, &texture),
        pixels.width,
        pixels.height,
        HostTextureAlphaMode::Premultiplied,
    );
    app.preview_gpu = Some(NativePreviewGpu { token, width: pixels.width, height: pixels.height });
}
