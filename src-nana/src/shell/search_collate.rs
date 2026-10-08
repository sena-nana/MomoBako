//! 筛选候选的排序，对应 Vue `localeCompare(…, "zh-CN")`。
//!
//! WebView2 的 `localeCompare` 走 ICU 的 zh 排序：标点符号在前，数字其次，汉字按拼音，
//! 然后才是拉丁字母（大小写不敏感、小写在前）。Windows 10 1903 起系统自带同一套 ICU
//! （`icu.dll`），这里按需加载它的 `ucol_strcoll`，排出来和 Vue 一致。
//! 加载不到时退回到同样分区的近似规则：分区内汉字按码位，其余按小写码位。

use std::cmp::Ordering;

/// 两个字符串按 zh-CN 排序比较。
pub(crate) fn compare_zh(left: &str, right: &str) -> Ordering {
    #[cfg(windows)]
    if let Some(order) = icu::compare(left, right) {
        return order;
    }
    fallback(left, right)
}

/// ICU 不可用时的近似：先按字符分区（标点、数字、汉字、拉丁、其他），再按小写码位，最后按原文。
fn fallback(left: &str, right: &str) -> Ordering {
    let key = |text: &str| -> Vec<(u8, u32)> {
        text.chars()
            .map(|ch| {
                let lower = ch.to_lowercase().next().unwrap_or(ch);
                (class_of(ch), lower as u32)
            })
            .collect()
    };
    key(left).cmp(&key(right)).then_with(|| right.cmp(left))
}

fn class_of(ch: char) -> u8 {
    if ch.is_ascii_digit() {
        1
    } else if is_han(ch) {
        2
    } else if ch.is_alphabetic() && (ch.is_ascii() || ('\u{00C0}'..='\u{024F}').contains(&ch)) {
        3
    } else if ch.is_alphanumeric() {
        4
    } else {
        0
    }
}

fn is_han(ch: char) -> bool {
    matches!(ch as u32, 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0x20000..=0x2FA1F)
}

#[cfg(windows)]
mod icu {
    //! 系统 `icu.dll` 的 zh 排序器。只加载一次，比较时加锁，不在多个线程上并发用同一个排序器。

    use std::cmp::Ordering;
    use std::ffi::c_void;
    use std::sync::{Mutex, OnceLock};

    type UcolOpen = unsafe extern "C" fn(locale: *const u8, status: *mut i32) -> *mut c_void;
    type UcolStrcoll = unsafe extern "C" fn(collator: *const c_void, source: *const u16, source_len: i32, target: *const u16, target_len: i32) -> i32;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn LoadLibraryW(name: *const u16) -> *mut c_void;
        fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
    }

    struct Collator {
        handle: usize,
        strcoll: UcolStrcoll,
    }

    static COLLATOR: OnceLock<Option<Mutex<Collator>>> = OnceLock::new();

    /// 用系统 ICU 比较。系统没有 `icu.dll` 或打不开 zh 排序器时返回 `None`。
    pub(super) fn compare(left: &str, right: &str) -> Option<Ordering> {
        let collator = COLLATOR.get_or_init(open).as_ref()?;
        let left: Vec<u16> = left.encode_utf16().collect();
        let right: Vec<u16> = right.encode_utf16().collect();
        let guard = match collator.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        let (Ok(left_len), Ok(right_len)) = (i32::try_from(left.len()), i32::try_from(right.len())) else {
            eprintln!("Nana 筛选候选过长，改用近似排序");
            return None;
        };
        // SAFETY：排序器在进程内一直有效；两段 UTF-16 缓冲在调用期间存活，长度是真实长度。
        let result = unsafe { (guard.strcoll)(guard.handle as *const c_void, left.as_ptr(), left_len, right.as_ptr(), right_len) };
        Some(result.cmp(&0))
    }

    fn open() -> Option<Mutex<Collator>> {
        let library: Vec<u16> = "icu.dll\0".encode_utf16().collect();
        // SAFETY：以 NUL 结尾的宽字符串；LoadLibraryW 失败返回空指针。
        let module = unsafe { LoadLibraryW(library.as_ptr()) };
        if module.is_null() {
            eprintln!("Nana 系统没有 icu.dll，筛选候选改用近似的 zh-CN 排序");
            return None;
        }
        // SAFETY：函数名以 NUL 结尾；找不到时返回空指针。
        let open = unsafe { GetProcAddress(module, b"ucol_open\0".as_ptr()) };
        let strcoll = unsafe { GetProcAddress(module, b"ucol_strcoll\0".as_ptr()) };
        if open.is_null() || strcoll.is_null() {
            eprintln!("Nana icu.dll 缺少 ucol_open 或 ucol_strcoll，筛选候选改用近似排序");
            return None;
        }
        // SAFETY：两个导出的签名与 ICU C API 一致。
        let (open, strcoll): (UcolOpen, UcolStrcoll) = unsafe { (std::mem::transmute(open), std::mem::transmute(strcoll)) };
        let mut status = 0i32;
        // SAFETY：区域名以 NUL 结尾；status 是有效的输出位置。
        let handle = unsafe { open(b"zh-CN\0".as_ptr(), &mut status) };
        // ICU 的负状态是警告（例如回退到 zh），正状态才是失败。
        if handle.is_null() || status > 0 {
            eprintln!("Nana 打不开 ICU zh 排序器（状态 {status}），筛选候选改用近似排序");
            return None;
        }
        Some(Mutex::new(Collator { handle: handle as usize, strcoll }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: [&str; 14] = ["png", "PNG", "参考", "#3fa796", "竖版", "jpg", "红色", "10", "封面", "9", "横版", "_x", "文档", "a"];

    fn sorted(compare: fn(&str, &str) -> Ordering) -> Vec<&'static str> {
        let mut items = SAMPLE.to_vec();
        items.sort_by(|left, right| compare(left, right));
        items
    }

    #[cfg(windows)]
    #[test]
    fn zh_order_puts_symbols_digits_han_then_latin() {
        assert_eq!(
            sorted(compare_zh),
            ["_x", "#3fa796", "10", "9", "参考", "封面", "横版", "红色", "竖版", "文档", "a", "jpg", "png", "PNG"]
        );
    }

    #[test]
    fn fallback_keeps_the_same_partitions() {
        let order = sorted(fallback);
        let position = |item: &str| order.iter().position(|value| *value == item).unwrap();
        assert!(position("#3fa796") < position("10"));
        assert!(position("10") < position("参考"));
        assert!(position("文档") < position("a"));
        assert!(position("png") < position("PNG"));
    }
}
