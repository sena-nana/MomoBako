//! 独立 ABI 插件宿主的产品边界验收。

use mutsuki_plugin_api::{
    ABI_V2_BRIDGE_ID, ABI_V2_CODEC_ID, ABI_V2_ENTRY_SYMBOL, ABI_V2_TRANSPORT_VERSION,
};

#[test]
fn momo_uses_the_published_abi_v2_surface() {
    assert_eq!(ABI_V2_TRANSPORT_VERSION, 2);
    assert_eq!(ABI_V2_ENTRY_SYMBOL, b"mutsuki_plugin_abi_v2\0");
    assert_eq!(ABI_V2_CODEC_ID, "mutsuki.codec.typed-msgpack.v1");
    assert_eq!(ABI_V2_BRIDGE_ID, "mutsuki.bridge.abi.binary.v2");
}
