// Written by art/make_crab.py: each part's rect in the crab sheets, Unity style (x, y from the bottom, w, h).
pub const SHEET: [u32; 2] = [48, 32];
pub const ICON: [u32; 2] = [48, 34];
pub const RECTS: &[(&str, [u32; 4])] = &[
    ("shell", [0, 16, 28, 16]),
    ("upper leg", [29, 24, 3, 8]),
    ("lower leg", [33, 20, 3, 12]),
    ("arm", [37, 24, 3, 8]),
    ("claw", [0, 5, 9, 10]),
    ("claw mirrored", [10, 5, 9, 10]),
    ("pincer", [20, 9, 4, 6]),
];
