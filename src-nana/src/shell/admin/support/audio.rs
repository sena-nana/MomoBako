//! 音频播放实现的选择视图与插件贡献的播放器。

use super::*;

/// 设置页里 Nana 内置解码器的名字。没有选中或官方的音频实现时，音频由它播放。
pub(super) const BUILTIN_AUDIO_NAME: &str = "内置解码器";

#[derive(Clone, Debug, PartialEq)]
pub struct AudioChoice {
    /// 选项的值。内置解码器不是插件，值为空串，选它等于清掉偏好。
    pub plugin_id: String,
    pub label: String,
}

/// 设置页音频播放器下拉框的数据。
#[derive(Clone, Debug, PartialEq)]
pub struct AudioView {
    pub choices: Vec<AudioChoice>,
    /// 下拉框当前值：偏好，其次解析出的播放器；由内置解码器播放时为空串。
    pub selected: String,
    /// 有插件或原生候选实现了音频能力才能选。
    pub selectable: bool,
    /// 回退提示（弱色）或缺失提示（危险色）。
    pub notice: Option<(String, bool)>,
}

/// 候选是音频能力的实现，按名称排序；偏好的实现不在候选里时先放一项「（不可用）」。
/// 标签是「插件名 · 插件标识」。实现取自已启用插件清单的 `playlistPlayers`（
/// Nana 用自己的音视频会话播放这些类型），以及登记了音频能力的原生候选。
///
/// Nana 自带内存解码器（WAV、MP3、FLAC、Ogg，Windows 上还有媒体基础解的 M4A、AAC、Opus），
/// 没有选中也没有官方实现时音频由它播放。这时下拉框写内置解码器、回退提示指向它，不报「音频播放暂不可用」；
/// 连内置解码器都没有才报缺失。
pub fn audio_view(
    candidates: &[PlayerCandidate],
    plugins: &[PluginManifest],
    preferences: &BTreeMap<String, String>,
) -> AudioView {
    let implementations = audio_implementations(candidates, plugins);
    let resolution_candidates: Vec<PlayerCandidate> = implementations
        .iter()
        .map(|(candidate, _)| candidate)
        .filter(|candidate| candidate.player_type_id == AUDIO_SEQUENCE_TYPE)
        .cloned()
        .collect();
    let resolution = resolve_player(AUDIO_SEQUENCE_TYPE, &resolution_candidates, preferences);
    let builtin = builtin_audio_formats(candidates)
        .map(|formats| format!("{BUILTIN_AUDIO_NAME} · {formats}"));
    let preferred = preferences.get(AUDIO_CAPABILITY).cloned();
    let mut choices = Vec::new();
    if let Some(plugin_id) = preferred.clone()
        && !implementations
            .iter()
            .any(|(candidate, _)| candidate.plugin_id == plugin_id)
    {
        choices.push(AudioChoice {
            plugin_id: plugin_id.clone(),
            label: format!("{plugin_id}（不可用）"),
        });
    }
    if resolution.player.is_none()
        && preferred.is_none()
        && let Some(label) = builtin.clone()
    {
        choices.push(AudioChoice {
            plugin_id: String::new(),
            label,
        });
    }
    choices.extend(implementations.iter().map(|(candidate, name)| AudioChoice {
        plugin_id: candidate.plugin_id.clone(),
        label: format!("{name} · {}", candidate.plugin_id),
    }));
    let selected = preferred.unwrap_or_else(|| {
        resolution
            .player
            .as_ref()
            .map(|player| player.plugin_id.clone())
            .unwrap_or_default()
    });
    let resolved_name = resolution.player.as_ref().and_then(|player| {
        implementations
            .iter()
            .find(|(candidate, _)| candidate == player)
            .map(|(_, name)| name.clone())
    });
    let notice = if resolution.fallback_used {
        let name = resolved_name
            .or_else(|| builtin.as_ref().map(|_| BUILTIN_AUDIO_NAME.to_string()))
            .unwrap_or_else(|| "官方默认实现".into());
        Some((format!("所选播放器当前不可用，已回退到 {name}。"), false))
    } else if resolution.player.is_none() && builtin.is_none() {
        Some((
            "官方音频播放器未启用或缺失，音频播放暂不可用。".into(),
            true,
        ))
    } else {
        None
    };
    AudioView {
        choices,
        selected,
        selectable: !implementations.is_empty(),
        notice,
    }
}

/// 实现音频能力的播放器和它所属插件的名字，按标签、插件名、插件标识排序。
/// 插件只取启用且状态不是 error 的，和后端登记播放器类型的口径一致，也就是 Nana 真能放的那些。
pub(super) fn audio_implementations(
    candidates: &[PlayerCandidate],
    plugins: &[PluginManifest],
) -> Vec<(PlayerCandidate, String)> {
    let plugin_name = |plugin_id: &str| {
        plugins
            .iter()
            .find(|plugin| plugin.plugin_id == plugin_id)
            .map(|plugin| plugin.name.clone())
    };
    let mut found: Vec<(PlayerCandidate, String)> = candidates
        .iter()
        .filter(|candidate| capability_id(candidate) == AUDIO_CAPABILITY)
        .map(|candidate| {
            (
                candidate.clone(),
                plugin_name(&candidate.plugin_id).unwrap_or_else(|| candidate.label.clone()),
            )
        })
        .collect();
    for plugin in plugins
        .iter()
        .filter(|plugin| plugin.enabled && plugin.status != "error")
    {
        for candidate in contributed_players(plugin) {
            let known = found.iter().any(|(item, _)| {
                item.plugin_id == candidate.plugin_id
                    && item.player_type_id == candidate.player_type_id
            });
            if !known && capability_id(&candidate) == AUDIO_CAPABILITY {
                found.push((candidate, plugin.name.clone()));
            }
        }
    }
    found.sort_by(|(left, left_name), (right, right_name)| {
        left.label
            .cmp(&right.label)
            .then(left_name.cmp(right_name))
            .then(left.plugin_id.cmp(&right.plugin_id))
    });
    found
}

/// 插件清单 `contributes.playlistPlayers` 里的播放器，带上清单里的 `capabilityId`。字段不全的条目记日志后跳过。
pub(super) fn contributed_players(plugin: &PluginManifest) -> Vec<PlayerCandidate> {
    let Some(entries) = plugin
        .contributes
        .get("playlistPlayers")
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };
    entries
        .iter()
        .filter_map(|entry| {
            match serde_json::from_value::<PlaylistPlayerContribution>(entry.clone()) {
                Ok(player) => Some(PlayerCandidate {
                    plugin_id: plugin.plugin_id.clone(),
                    player_type_id: player.player_type_id,
                    capability_id: entry
                        .get("capabilityId")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    label: player.label,
                    file_class: player.file_class,
                    extensions: player.supported_extensions,
                    supports_seek: player.supports_seek,
                    supports_volume: player.supports_volume,
                }),
                Err(error) => {
                    eprintln!(
                        "Nana 插件 {} 的播放器条目无法读取：{error}",
                        plugin.plugin_id
                    );
                    None
                }
            }
        })
        .collect()
}
