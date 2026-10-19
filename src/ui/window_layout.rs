//! Shared geometry and production visual tuning for building and army windows.
use macroquad::prelude::*;
use super::chrome;
pub const WIDTH: f32 = 836.0;
pub const HEIGHT: f32 = 600.0;
const DIVIDER: f32 = 28.7;
#[derive(Clone, Copy)]
pub struct Visual {
    pub quest_brightness: f32, pub quest_gamma: f32, pub quest_opacity: f32, pub quest_cutoff: f32,
    pub brown_brightness: f32, pub brown_gamma: f32,
    pub white_brightness: f32, pub white_gamma: f32, pub white_ornament: f32,
    pub paper_edge_width: f32, pub paper_edge_opacity: f32, pub quest_text_saturation: f32,
    pub bar_ornament_brightness: f32, pub bar_ornament_gamma: f32, pub bar_ornament_opacity: f32,
    pub bar_gradient_bottom: f32,
}
pub fn visual() -> Visual {
    Visual {
        quest_brightness: 1.070, quest_gamma: 1.167, quest_opacity: 1.0, quest_cutoff: 0.0,
        brown_brightness: 1.057, brown_gamma: 0.805,
        white_brightness: 1.007, white_gamma: 0.914, white_ornament: 0.576,
        paper_edge_width: 3.724, paper_edge_opacity: 0.400, quest_text_saturation: 0.874,
        bar_ornament_brightness: 2.000, bar_ornament_gamma: 0.904,
        bar_ornament_opacity: 1.000, bar_gradient_bottom: 0.388,
    }
}
fn upper_height() -> f32 { (570.0 - DIVIDER) / 2.0 }
fn lower_y() -> f32 { 28.0 + upper_height() + DIVIDER }
fn lower_height() -> f32 { upper_height() }
pub fn window() -> Rect {
    let k = chrome::k();
    let (w, h) = ((WIDTH*k).round(), (HEIGHT*k).round());
    Rect::new(((screen_width()-w)/2.0).round(), ((screen_height()-chrome::bar_height()-h)/2.0).max(2.0).round(), w, h)
}
/// Map design coordinates to the fixed, symmetric upper and lower panes.
pub fn at(win: Rect, x: f32, y: f32, w: f32, h: f32) -> Rect {
    let k = chrome::k();
    let (yy,hh) = if y >= 326.0 { let s=lower_height()/272.0; (lower_y()+(y-326.0)*s,h*s) }
    else if y >= 300.0 { let s=DIVIDER/26.0; (28.0+upper_height()+(y-300.0)*s,h*s) }
    else if y >= 28.0 { let s=upper_height()/272.0; (28.0+(y-28.0)*s,h*s) }
    else {(y,h)};
    Rect::new(win.x+x*k,win.y+yy*k,w*k,hh*k)
}
pub fn left(win: Rect) -> Rect { let k=chrome::k(); Rect::new(win.x+2.0*k,win.y+27.0*k,244.0*k,571.0*k) }
/// Shared structural grid, drawn last so tab artwork cannot erase its borders.
pub fn frame(win: Rect) {
    let k = chrome::k();
    chrome::silver_frame(win, 2.0 * k);
    let x = win.x + 247.0 * k;
    chrome::silver_line(vec2(x, win.y + 26.0 * k), vec2(x, win.bottom() - 2.0 * k), 2.0 * k);
    let strip = divider(win);
    for y in [strip.y + k, strip.bottom() - k] {
        chrome::silver_line(vec2(x, y), vec2(win.right() - 2.0 * k, y), 2.0 * k);
    }
}
pub fn panel_frame(r: Rect) {
    let gap = 2.0 * chrome::k();
    chrome::silver_frame(Rect::new(r.x + gap, r.y + gap, r.w - 2.0 * gap, r.h - 2.0 * gap), chrome::k());
}
pub fn upper(win: Rect) -> Rect { let k=chrome::k(); Rect::new(win.x+248.0*k,win.y+28.0*k,586.0*k,upper_height()*k) }
pub fn divider(win: Rect) -> Rect { let k=chrome::k(); Rect::new(win.x+248.0*k,win.y+(28.0+upper_height())*k,586.0*k,DIVIDER*k) }
pub fn lower(win: Rect) -> Rect { let k=chrome::k(); Rect::new(win.x+248.0*k,win.y+lower_y()*k,586.0*k,lower_height()*k) }
pub fn lower_content(win: Rect) -> Rect { let r=lower(win); let k=chrome::k(); Rect::new(r.x+8.0*k,r.y+8.0*k,r.w-14.0*k,r.h-16.0*k) }
pub fn quest_rects(panel: Rect) -> (Rect,Rect) {
    let map=|x:f32,y:f32,w:f32,h:f32| Rect::new(panel.x+x*panel.w,panel.y+y*panel.h,w*panel.w,h*panel.h);
    (map(0.0000,0.0000,0.7389,0.1989),map(0.7471,0.0313,0.2459,0.1440))
}
pub fn army_grid_left(panel: Rect, columns: u8, pitch: f32, trailing: f32) -> f32 {
    (panel.x+(panel.w-(columns as f32*pitch-trailing))/2.0).round()
}
