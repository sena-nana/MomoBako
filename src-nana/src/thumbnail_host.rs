//! 把解码后的缩略图像素登记成宿主纹理。
//!
//! 像素不放进会整份克隆的壳层状态。布局只用原始宽高，绘制用缩小后的纹理。

use nana_ui::{
    ApplicationWindow, GpuTexture, GpuTextureDescriptor, GpuTextureFormat, GpuTextureRegion, GpuTextureUsages,
    HostTexture, HostTextureAlphaMode, RuntimeProgramContext,
};

use crate::shell::{thumbnail_slot, ThumbnailFrame};
use crate::MomoBakoApplication;

pub(crate) struct ThumbnailGpu {
    pub(crate) slot: String,
    texture: GpuTexture,
    width: u32,
    height: u32,
}

pub(crate) struct PendingThumb {
    slot: String,
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

impl MomoBakoApplication {
    /// 从解码结果里拿走像素，留给下一帧上传。宽高仍留在消息里给布局用。
    pub(crate) fn queue_thumbnail_frames(&mut self, frames: &mut [ThumbnailFrame]) {
        for frame in frames {
            let rgba = std::mem::take(&mut frame.rgba);
            if rgba.len() != (frame.width as usize).saturating_mul(frame.height as usize).saturating_mul(4) {
                eprintln!("Nana 缩略图像素长度不对：{}", frame.path);
                continue;
            }
            self.pending_thumbs.push(PendingThumb {
                slot: thumbnail_slot(&frame.path),
                width: frame.width,
                height: frame.height,
                rgba,
            });
        }
    }

    /// 上传新缩略图，并在每帧重新登记已有纹理。
    pub(crate) fn publish_thumbnails(
        &mut self,
        window: &mut ApplicationWindow,
        context: &RuntimeProgramContext<crate::shell::ShellMessage>,
    ) {
        for pending in std::mem::take(&mut self.pending_thumbs) {
            let Ok(texture) = context.gpu().create_texture(&GpuTextureDescriptor {
                label: Some("momobako thumbnail"),
                width: pending.width,
                height: pending.height,
                format: GpuTextureFormat::RGBA8_UNORM_SRGB,
                usage: GpuTextureUsages::SAMPLED | GpuTextureUsages::COPY_DST,
            }) else {
                eprintln!("Nana 缩略图纹理创建失败：{}x{}", pending.width, pending.height);
                continue;
            };
            if let Err(error) = context.gpu().write_texture(
                &texture,
                GpuTextureRegion::full(pending.width, pending.height),
                &pending.rgba,
                pending.width.saturating_mul(4),
            ) {
                eprintln!("Nana 缩略图纹理上传失败：{error}");
                continue;
            }
            self.thumbnail_gpu.retain(|item| item.slot != pending.slot);
            self.thumbnail_gpu.push(ThumbnailGpu {
                slot: pending.slot,
                texture,
                width: pending.width,
                height: pending.height,
            });
        }
        if self.thumbnail_gpu.len() > 240 {
            let extra = self.thumbnail_gpu.len() - 240;
            self.thumbnail_gpu.drain(0..extra);
        }
        for thumb in &self.thumbnail_gpu {
            window.textures.register(
                thumb.slot.clone(),
                HostTexture::new(1, 1, &thumb.texture),
                thumb.width,
                thumb.height,
                HostTextureAlphaMode::Premultiplied,
            );
        }
    }
}
