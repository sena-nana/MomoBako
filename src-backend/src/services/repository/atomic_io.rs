//! 可取消文件写入的原子发布工具。

use super::*;

/// 分块写入同目录临时文件，完整写入并完成发布准备后再重命名。
pub(super) fn publish_reader_atomically(
    source: &mut dyn Read,
    target: &Path,
    cancellation: &dyn CancellationCheck,
    prepare_publish: impl FnOnce(&Path) -> Result<(), String>,
) -> Result<(), String> {
    let file_name = target
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("entry");
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temporary = target.with_file_name(format!(
        ".{file_name}.momobako-part-{}-{nonce}",
        std::process::id()
    ));
    let result = (|| {
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(io_error)?;
        let mut buffer = vec![0_u8; 1024 * 1024];
        loop {
            cancellation.checkpoint()?;
            let read = source.read(&mut buffer).map_err(io_error)?;
            if read == 0 {
                break;
            }
            output.write_all(&buffer[..read]).map_err(io_error)?;
        }
        output.flush().map_err(io_error)?;
        cancellation.checkpoint()?;
        prepare_publish(&temporary)?;
        fs::rename(&temporary, target).map_err(io_error)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}
