use slot_ui::{ease, out_w, Draw, Millis, TexId, ARROW_LEFT_X, ARROW_RIGHT_X, ARROW_Y};
use slot_ui::link_hardware::{Hardware, BAR_Y};
use crate::app::{GameMenu, LinkRow, ReleaseFrom};

#[derive(Copy, Clone)]
pub struct Sprite {
    pub tex: TexId,
    pub w: u32,
    pub h: u32,
}

pub struct Marks {
    pub arcs_right: [Sprite; 3],
    pub arcs_left: [Sprite; 3],
    pub clicks: Sprite,
    pub arrow_left: Sprite,
    pub arrow_right: Sprite,
}

pub struct LinkSprites {
    pub key: Hardware,
    pub wall: Sprite,
    pub front: Sprite,
    pub socket: Sprite,
    pub rim: Sprite,
    pub contact: Sprite,
    pub grips: [Sprite; 2],
    pub nose: Sprite,
    pub buttons: Option<[[Sprite; 2]; 2]>,
}

const DROP_MS: Millis = 200;
const SEAT_MS: Millis = 160;
const LIFT_MS: Millis = 250;
const UNPLUG_MS: Millis = 260;
const RFU_RELEASE_MS: Millis = 60;
const READY_TIP: f32 = 385.0;

fn eased(now: Millis, since: Millis, ms: Millis) -> f32 {
    ease((now.saturating_sub(since) as f32 / ms as f32).clamp(0.0, 1.0))
}

fn lerp(a: f32, b: f32, t: f32) -> f32 { if t >= 1.0 { b } else { a + (b - a) * t } }

fn failed_travel(worked: Millis, since: Millis, now: Millis, wireless: bool) -> f32 {
    let at = eased(since, worked, DROP_MS);
    if wireless { at } else { lerp(at, -0.35, eased(now, since, LIFT_MS)) }
}

pub fn travel(menu: GameMenu, now: Millis, wireless: bool) -> f32 {
    match menu {
        GameMenu::Pick(_) => 0.0,
        GameMenu::Working { since, .. } => eased(now, since, DROP_MS),
        GameMenu::Linked { opened: true, .. } => 1.0,
        GameMenu::Linked { worked, since, .. } => lerp(eased(since, worked, DROP_MS), 1.0, eased(now, since, SEAT_MS)),
        GameMenu::Failed { worked, since, .. } => failed_travel(worked, since, now, wireless),
        GameMenu::Unplug { from, since, .. } => {
            let initial = match from {
                ReleaseFrom::Seated => 1.0,
                ReleaseFrom::Search { worked } => eased(since, worked, DROP_MS),
                ReleaseFrom::Failure { worked, failed } => failed_travel(worked, failed, since, wireless),
            };
            let release = if wireless { RFU_RELEASE_MS } else { 0 };
            lerp(initial, 0.0, eased(now, since + release, UNPLUG_MS))
        }
    }
}

fn grip_alpha(menu: GameMenu, now: Millis, wireless: bool) -> f32 {
    if wireless { return 1.0; }
    match menu {
        GameMenu::Failed { since, .. } => lerp(1.0, 0.52, eased(now, since, LIFT_MS)),
        GameMenu::Unplug { from: ReleaseFrom::Failure { failed, .. }, since, .. } =>
            lerp(lerp(1.0, 0.52, eased(since, failed, LIFT_MS)), 1.0, eased(now, since, UNPLUG_MS)),
        _ => 1.0,
    }
}

fn search_arcs(now: Millis, since: Millis) -> [f32; 3] {
    [0, 1, 2].map(|i| {
        let Some(dt) = now.saturating_sub(since).checked_sub(DROP_MS + 300 * i) else { return 0.0 };
        1.0 - (2.0 * (dt as f32 / 1200.0).fract() - 1.0).abs()
    })
}

pub fn arc_alphas(menu: GameMenu, now: Millis) -> [f32; 3] {
    match menu {
        GameMenu::Pick(_) => [0.0; 3],
        GameMenu::Working { since, .. } => search_arcs(now, since),
        GameMenu::Linked { opened: true, .. } => [1.0; 3],
        GameMenu::Linked { worked, since, .. } => search_arcs(since, worked).map(|a| lerp(a, 1.0, eased(now, since, SEAT_MS))),
        GameMenu::Failed { worked, since, .. } => search_arcs(since, worked).map(|a| a * (1.0 - eased(now, since, LIFT_MS))),
        GameMenu::Unplug { from, since, .. } => {
            let initial = match from {
                ReleaseFrom::Seated => [1.0; 3],
                ReleaseFrom::Search { worked } => search_arcs(since, worked),
                ReleaseFrom::Failure { worked, failed } => search_arcs(failed, worked).map(|a| a * (1.0 - eased(since, failed, LIFT_MS))),
            };
            initial.map(|a| a * (1.0 - eased(now, since, RFU_RELEASE_MS)))
        }
    }
}

pub fn clicks_alpha(menu: GameMenu, now: Millis) -> f32 {
    match menu {
        GameMenu::Linked { opened: true, .. } => 1.0,
        GameMenu::Linked { since, .. } => eased(now, since, SEAT_MS),
        _ => 0.0,
    }
}

fn tex(out: &mut Vec<Draw>, s: Sprite, x: f32, y: f32, alpha: f32) {
    out.push(Draw::Tex { x, y, w: s.w as f32, h: s.h as f32, tex: s.tex, alpha });
}

pub fn draw_link_art(menu: GameMenu, now: Millis, s: &LinkSprites, marks: &Marks, out: &mut Vec<Draw>) {
    let role = match menu {
        GameMenu::Pick(role) | GameMenu::Working { role, .. } | GameMenu::Linked { role, .. } |
        GameMenu::Failed { role, .. } | GameMenu::Unplug { role, .. } => role,
    };
    let key = s.key;
    let join = role == LinkRow::Join;
    let centre = out_w() as f32 * 0.5;
    let x = centre + key.offset(join);
    let grip = s.grips[role.index()];
    let height = grip.h as f32;
    let seated = BAR_Y + height - key.shoulder();
    let ready = READY_TIP - key.travel_offset();
    let tip = lerp(ready, seated, travel(menu, now, key.wireless));
    let y = tip - height;
    let left = x - grip.w as f32 / 2.0;
    let alpha = grip_alpha(menu, now, key.wireless);
    let sx = x - s.socket.w as f32 / 2.0;
    tex(out, s.wall, 0.0, BAR_Y, 1.0);
    tex(out, s.socket, sx, BAR_Y, 1.0);
    if key.dual() {
        tex(out, s.socket, centre + key.offset(!join) - s.socket.w as f32 / 2.0, BAR_Y, 1.0);
    }
    tex(out, s.nose, left, y, alpha);
    // The fixed, perforated fascia covers the shank outside the receiver.
    // Its mouth, lip and remaining port never disappear as the plug seats.
    tex(out, s.front, 0.0, BAR_Y, 1.0);
    tex(out, s.rim, sx, BAR_Y, 1.0);
    if key.dual() {
        tex(out, s.rim, centre + key.offset(!join) - s.rim.w as f32 / 2.0, BAR_Y, 1.0);
    }
    let contact = ease(((9.0-(seated-tip))/9.0).clamp(0.0,1.0))*alpha;
    tex(out, s.contact, left, BAR_Y, contact);
    if let Some(buttons) = s.buttons.as_ref().filter(|_| key.wireless) {
        let pressure = match menu {
            GameMenu::Unplug { since, .. } => eased(now, since, RFU_RELEASE_MS) * (1.0 - eased(now, since + 205, 115)),
            _ => 0.0,
        };
        let [l, r] = buttons[role.index()];
        tex(out, l, left + 22.0 + pressure * 2.8, y + 28.0, 1.0);
        tex(out, r, left + 178.0 - pressure * 2.8, y + 28.0, 1.0);
    }
    tex(out, grip, left, y, alpha);
    if key.wireless {
        for (i, a) in arc_alphas(menu, now).into_iter().enumerate() {
            if a <= 0.0 { continue; }
            let dx = [120.0, 133.0, 146.0][i];
            let (r, l) = (marks.arcs_right[i], marks.arcs_left[i]);
            let ay = y + 79.0 - r.h as f32 / 2.0;
            tex(out, r, x + dx - 2.0, ay, a);
            tex(out, l, x - dx + 2.0 - l.w as f32, ay, a);
        }
    } else {
        let a = clicks_alpha(menu, now);
        if a > 0.0 {
            let y = BAR_Y - if key.side_exit() { 41.0 + key.travel_offset() } else { 31.0 };
            tex(out, marks.clicks, x - marks.clicks.w as f32 / 2.0, y, a);
        }
    }
    if matches!(menu, GameMenu::Pick(_)) {
        let dx = centre - slot_gfx::BASE_W as f32 / 2.0;
        tex(out, marks.arrow_left, ARROW_LEFT_X + dx, ARROW_Y, 0.7);
        tex(out, marks.arrow_right, ARROW_RIGHT_X + dx, ARROW_Y, 0.7);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{LinkFail,LinkStep};

    #[test]
    fn moving_the_receiver_preserves_the_full_motion_and_speed() {
        for family in 0..7 {
            for wireless in [false,true] {
                let key = Hardware::new(family,0,wireless);
                let height = if key.wireless {151.} else {244.};
                let old_seat = BAR_Y+key.travel_offset()+height-key.shoulder();
                let new_seat = BAR_Y+height-key.shoulder();
                let new_ready = READY_TIP-key.travel_offset();
                for dt in [0,25,50,100,150,199,200,1000] {
                    let menu = GameMenu::Working { role:LinkRow::Host, step:LinkStep::Waiting, since:100 };
                    let t = travel(menu,100+dt,key.wireless);
                    let old_tip = lerp(READY_TIP,old_seat,t);
                    let new_tip = lerp(new_ready,new_seat,t);
                    assert!((old_tip-new_tip-key.travel_offset()).abs()<0.0001);
                    if dt==100 { assert_eq!(t,0.5); }
                    if dt>=200 { assert_eq!(t,1.); }
                }
            }
        }
    }

    #[test]
    fn clicks_require_a_real_link_and_remain_visible_when_its_menu_reopens() {
        for role in LinkRow::ALL {
            let linked = GameMenu::Linked { role,worked:0,since:100,opened:false };
            assert_eq!(clicks_alpha(linked,100),0.);
            assert_eq!(clicks_alpha(linked,180),0.5);
            assert_eq!(clicks_alpha(linked,260),1.);
            assert_eq!(clicks_alpha(linked,100_000),1.);
            let reopened = GameMenu::Linked {role,worked:0,since:100_000,opened:true};
            assert_eq!(travel(reopened,100_000,false),1.);
            assert_eq!(clicks_alpha(reopened,100_000),1.);
            for menu in [
                GameMenu::Pick(role),
                GameMenu::Working {role,step:LinkStep::Radio,since:0},
                GameMenu::Working {role,step:LinkStep::Waiting,since:0},
                GameMenu::Failed {role,fail:LinkFail::PeerVanished,worked:0,since:200},
                GameMenu::Unplug {role,since:200,from:ReleaseFrom::Seated},
            ] {
                assert_eq!(clicks_alpha(menu,1000),0.);
            }
        }
    }

    #[test]
    fn cancelling_search_or_failure_withdraws_without_a_position_jump() {
        for wireless in [false,true] {
            let role = LinkRow::Join;
            let search = GameMenu::Working {role,step:LinkStep::Waiting,since:100};
            let failed = GameMenu::Failed {role,fail:LinkFail::NobodyCame,worked:100,since:190};
            for (previous,from,since) in [
                (search,ReleaseFrom::Search {worked:100},150),
                (failed,ReleaseFrom::Failure {worked:100,failed:190},280),
            ] {
                let unplug = GameMenu::Unplug {role,since,from};
                let initial = travel(previous,since,wireless);
                assert_eq!(travel(unplug,since,wireless),initial);
                let delay = if wireless {60} else {0};
                assert_eq!(travel(unplug,since+delay,wireless),initial);
                assert!((travel(unplug,since+delay+130,wireless)-initial/2.).abs()<0.00001);
                assert_eq!(travel(unplug,since+delay+260,wireless),0.);
            }
        }
    }
}
