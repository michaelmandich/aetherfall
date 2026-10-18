//! Player notices without spam.

use af_core::prelude::*;
use bevy::prelude::*;

use crate::tuning::NOTICE_COOLDOWN;

/// Tell `player` something, unless the same text reached them a moment ago
/// (a dozen workers giving up at once should say it once).
pub(crate) fn notify(notices: &mut Notices, player: PlayerId, kind: NoticeKind, text: &str, pos: Option<Vec2>, now: f32) {
    let repeated = notices
        .list
        .iter()
        .rev()
        .take_while(|n| now - n.time < NOTICE_COOLDOWN)
        .any(|n| n.player == Some(player) && n.text == text);
    if !repeated {
        notices.push(Some(player), kind, text, pos, now);
    }
}
