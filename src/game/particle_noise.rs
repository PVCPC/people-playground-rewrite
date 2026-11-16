//! NoiseModule (UnityPlayer 2020.3): the per-run scroll, the position offset and CalculateNoiseJob<D, octaves,
//! separate> of one particle (lane). The Perlin samplers are lane 0 of Perlin1D (180dddec0), Perlin2D (180dde180) and
//! Perlin3D (180dde9f0), translated op for op from the SSE code by a symbolic run of the disassembly, so the
//! association of every add and multiply is the native one.
#![allow(clippy::all, unused_variables)]
use crate::assets::particles::Noise;
use crate::game::particle_modules::{draws, V3};
use crate::game::particle_sim::{evaluate, inverse, maxps};

const P: [i32; 512] = [151, 160, 137, 91, 90, 15, 131, 13, 201, 95, 96, 53, 194, 233, 7, 225, 140, 36, 103, 30, 69, 142, 8, 99, 37, 240, 21, 10, 23, 190, 6, 148, 247, 120, 234, 75, 0, 26, 197, 62, 94, 252, 219, 203, 117, 35, 11, 32, 57, 177, 33, 88, 237, 149, 56, 87, 174, 20, 125, 136, 171, 168, 68, 175, 74, 165, 71, 134, 139, 48, 27, 166, 77, 146, 158, 231, 83, 111, 229, 122, 60, 211, 133, 230, 220, 105, 92, 41, 55, 46, 245, 40, 244, 102, 143, 54, 65, 25, 63, 161, 1, 216, 80, 73, 209, 76, 132, 187, 208, 89, 18, 169, 200, 196, 135, 130, 116, 188, 159, 86, 164, 100, 109, 198, 173, 186, 3, 64, 52, 217, 226, 250, 124, 123, 5, 202, 38, 147, 118, 126, 255, 82, 85, 212, 207, 206, 59, 227, 47, 16, 58, 17, 182, 189, 28, 42, 223, 183, 170, 213, 119, 248, 152, 2, 44, 154, 163, 70, 221, 153, 101, 155, 167, 43, 172, 9, 129, 22, 39, 253, 19, 98, 108, 110, 79, 113, 224, 232, 178, 185, 112, 104, 218, 246, 97, 228, 251, 34, 242, 193, 238, 210, 144, 12, 191, 179, 162, 241, 81, 51, 145, 235, 249, 14, 239, 107, 49, 192, 214, 31, 181, 199, 106, 157, 184, 84, 204, 176, 115, 121, 50, 45, 127, 4, 150, 254, 138, 236, 205, 93, 222, 114, 67, 29, 24, 72, 243, 141, 128, 195, 78, 66, 215, 61, 156, 180, 151, 160, 137, 91, 90, 15, 131, 13, 201, 95, 96, 53, 194, 233, 7, 225, 140, 36, 103, 30, 69, 142, 8, 99, 37, 240, 21, 10, 23, 190, 6, 148, 247, 120, 234, 75, 0, 26, 197, 62, 94, 252, 219, 203, 117, 35, 11, 32, 57, 177, 33, 88, 237, 149, 56, 87, 174, 20, 125, 136, 171, 168, 68, 175, 74, 165, 71, 134, 139, 48, 27, 166, 77, 146, 158, 231, 83, 111, 229, 122, 60, 211, 133, 230, 220, 105, 92, 41, 55, 46, 245, 40, 244, 102, 143, 54, 65, 25, 63, 161, 1, 216, 80, 73, 209, 76, 132, 187, 208, 89, 18, 169, 200, 196, 135, 130, 116, 188, 159, 86, 164, 100, 109, 198, 173, 186, 3, 64, 52, 217, 226, 250, 124, 123, 5, 202, 38, 147, 118, 126, 255, 82, 85, 212, 207, 206, 59, 227, 47, 16, 58, 17, 182, 189, 28, 42, 223, 183, 170, 213, 119, 248, 152, 2, 44, 154, 163, 70, 221, 153, 101, 155, 167, 43, 172, 9, 129, 22, 39, 253, 19, 98, 108, 110, 79, 113, 224, 232, 178, 185, 112, 104, 218, 246, 97, 228, 251, 34, 242, 193, 238, 210, 144, 12, 191, 179, 162, 241, 81, 51, 145, 235, 249, 14, 239, 107, 49, 192, 214, 31, 181, 199, 106, 157, 184, 84, 204, 176, 115, 121, 50, 45, 127, 4, 150, 254, 138, 236, 205, 93, 222, 114, 67, 29, 24, 72, 243, 141, 128, 195, 78, 66, 215, 61, 156, 180];
/// s_Gradients1D (0x181732fd0).
const G1: [f32; 2] = [1.0, -1.0];
/// s_Gradients2D (0x1819a0700) as (x, y) pairs; the last four are 1 / sqrtf(2) with signs, set by 180084230.
const R: f32 = f32::from_bits(0x3f3504f3);
const G2: [f32; 16] = [1.0, 0.0, -1.0, 0.0, 0.0, 1.0, 0.0, -1.0, R, R, -R, R, R, -R, -R, -R];
/// s_Gradients3D (0x1819a0740), LoadGradients3D (180dddd30) indexes it with P[i] & 15.
const G3: [[f32; 3]; 16] = [
    [1.0, 1.0, 0.0], [-1.0, 1.0, 0.0], [1.0, -1.0, 0.0], [-1.0, -1.0, 0.0],
    [1.0, 0.0, 1.0], [-1.0, 0.0, 1.0], [1.0, 0.0, -1.0], [-1.0, 0.0, -1.0],
    [0.0, 1.0, 1.0], [0.0, -1.0, 1.0], [0.0, 1.0, -1.0], [0.0, -1.0, -1.0],
    [1.0, 1.0, 0.0], [-1.0, 1.0, 0.0], [0.0, -1.0, 1.0], [0.0, -1.0, -1.0],
];

fn perlin1d(c: [f32; 3], f: f32) -> [f32; 2] {
    let t207 = 2.0 * f;
    let t10 = c[0] * f;
    let t14 = t10.round_ties_even() as i32;
    let t18 = t14 as f32;
    let t27 = if t10 < t18 { 1.0 } else { 0.0 };
    let t31 = t18 - t27;
    let t39 = t31 as i32;
    let t45 = t39 & 255;
    let t53 = 1i32.wrapping_add(t45);
    let t79 = P[t53 as usize];
    let t80 = t79 & 1;
    let t81 = G1[t80 as usize];
    let t35 = t10 - t31;
    let t57 = t35 - 1.0;
    let t135 = t81 * t57;
    let t66 = P[t45 as usize];
    let t68 = t66 & 1;
    let t69 = G1[t68 as usize];
    let t159 = t35 * t69;
    let t171 = t135 - t159;
    let t122 = 30.0 * t35;
    let t147 = t122 * t35;
    let t49 = t35 - 2.0;
    let t62 = t49 * t35;
    let t91 = t62 + 1.0;
    let t163 = t147 * t91;
    let t183 = t171 * t163;
    let t191 = t183 + t69;
    let t151 = t81 - t69;
    let t126 = t35 * t35;
    let t155 = t126 * t35;
    let t97 = 6.0 * t35;
    let t106 = t97 + -15.0;
    let t118 = t35 * t106;
    let t131 = t118 + 10.0;
    let t175 = t155 * t131;
    let t187 = t151 * t175;
    let t199 = t191 + t187;
    let t211 = t207 * t199;
    [t211, 0.0]
}

fn perlin2d(c: [f32; 3], f: f32) -> [f32; 2] {
    let t542 = f32::from_bits(0x3fb504f3) * f;
    let t9 = f * c[1];
    let t21 = t9.round_ties_even() as i32;
    let t25 = t21 as f32;
    let t34 = if t9 < t25 { 1.0 } else { 0.0 };
    let t38 = t25 - t34;
    let t46 = t9 - t38;
    let t331 = t46 * t46;
    let t361 = t331 * t46;
    let t344 = t46 * 6.0;
    let t348 = t344 + -15.0;
    let t352 = t46 * t348;
    let t356 = t352 + 10.0;
    let t373 = t361 * t356;
    let t74 = t46 - 1.0;
    let t17 = f * c[0];
    let t42 = t17.round_ties_even() as i32;
    let t50 = t42 as f32;
    let t58 = if t17 < t50 { 1.0 } else { 0.0 };
    let t62 = t50 - t58;
    let t78 = t62 as i32;
    let t87 = t78 & 255;
    let t97 = t87.wrapping_add(1);
    let t109 = P[t97 as usize];
    let t82 = t38 as i32;
    let t91 = t82 & 255;
    let t101 = t91.wrapping_add(1);
    let t170 = t109.wrapping_add(t101);
    let t186 = P[t170 as usize];
    let t187 = t186 & 7;
    let t189 = G2[t187 as usize * 2 + 1];
    let t393 = t74 * t189;
    let t66 = t17 - t62;
    let t70 = t66 - 1.0;
    let t188 = G2[t187 as usize * 2 + 0];
    let t401 = t70 * t188;
    let t417 = t393 + t401;
    let t105 = P[t87 as usize];
    let t155 = t101.wrapping_add(t105);
    let t163 = P[t155 as usize];
    let t164 = t163 & 7;
    let t174 = G2[t164 as usize * 2 + 1];
    let t259 = t74 * t174;
    let t165 = G2[t164 as usize * 2 + 0];
    let t277 = t165 * t66;
    let t289 = t259 + t277;
    let t429 = t417 - t289;
    let t132 = t109.wrapping_add(t91);
    let t144 = P[t132 as usize];
    let t145 = t144 & 7;
    let t147 = G2[t145 as usize * 2 + 1];
    let t233 = t46 * t147;
    let t146 = G2[t145 as usize * 2 + 0];
    let t245 = t70 * t146;
    let t249 = t233 + t245;
    let t445 = t429 - t249;
    let t116 = t91.wrapping_add(t105);
    let t125 = P[t116 as usize];
    let t127 = t125 & 7;
    let t128 = G2[t127 as usize * 2 + 1];
    let t202 = t46 * t128;
    let t129 = G2[t127 as usize * 2 + 0];
    let t215 = t129 * t66;
    let t219 = t202 + t215;
    let t453 = t445 + t219;
    let t481 = t373 * t453;
    let t449 = t249 - t219;
    let t493 = t481 + t449;
    let t255 = 30.0 * t66;
    let t264 = t255 * t66;
    let t269 = t66 - 2.0;
    let t281 = t269 * t66;
    let t297 = t281 + 1.0;
    let t369 = t264 * t297;
    let t505 = t493 * t369;
    let t301 = t66 * t66;
    let t318 = t301 * t66;
    let t310 = 6.0 * t66;
    let t323 = t310 + -15.0;
    let t327 = t323 * t66;
    let t336 = t327 + 10.0;
    let t340 = t318 * t336;
    let t457 = t146 - t129;
    let t473 = t340 * t457;
    let t469 = t165 - t129;
    let t405 = t188 - t165;
    let t413 = t405 - t146;
    let t425 = t413 + t129;
    let t437 = t340 * t425;
    let t477 = t469 + t437;
    let t489 = t373 * t477;
    let t501 = t489 + t129;
    let t509 = t473 + t501;
    let t521 = t505 + t509;
    let t578 = t542 * t521;
    let t513 = t340 * t453;
    let t433 = t289 - t219;
    let t525 = t513 + t433;
    let t273 = t46 * 30.0;
    let t285 = t273 * t46;
    let t293 = t46 - 2.0;
    let t305 = t46 * t293;
    let t314 = t305 + 1.0;
    let t381 = t285 * t314;
    let t534 = t525 * t381;
    let t465 = t147 - t128;
    let t517 = t340 * t465;
    let t388 = t174 - t128;
    let t397 = t189 - t174;
    let t409 = t397 - t147;
    let t421 = t128 + t409;
    let t441 = t340 * t421;
    let t461 = t388 + t441;
    let t485 = t373 * t461;
    let t497 = t485 + t128;
    let t529 = t517 + t497;
    let t538 = t534 + t529;
    let t586 = t542 * t538;
    [t578, t586]
}

fn perlin3d(c: [f32; 3], f: f32) -> [f32; 2] {
    let t19 = c[1] * f;
    let t51 = t19.round_ties_even() as i32;
    let t59 = t51 as f32;
    let t71 = if t19 < t59 { 1.0 } else { 0.0 };
    let t75 = t59 - t71;
    let t83 = t19 - t75;
    let t684 = t83 * t83;
    let t692 = t684 * t83;
    let t672 = t83 * 6.0;
    let t676 = t672 + -15.0;
    let t680 = t83 * t676;
    let t688 = t680 + 10.0;
    let t696 = t692 * t688;
    let t103 = t83 - 1.0;
    let t27 = f * c[0];
    let t63 = t27.round_ties_even() as i32;
    let t79 = t63 as f32;
    let t91 = if t27 < t79 { 1.0 } else { 0.0 };
    let t95 = t79 - t91;
    let t111 = t95 as i32;
    let t128 = t111 & 255;
    let t132 = 1i32.wrapping_add(t128);
    let t165 = P[t132 as usize];
    let t123 = t75 as i32;
    let t136 = t123 & 255;
    let t144 = 1i32.wrapping_add(t136);
    let t188 = t165.wrapping_add(t144);
    let t199 = P[t188 as usize];
    let t15 = c[2] * f;
    let t31 = t15.round_ties_even() as i32;
    let t35 = t31 as f32;
    let t43 = if t15 < t35 { 1.0 } else { 0.0 };
    let t47 = t35 - t43;
    let t99 = t47 as i32;
    let t140 = t99 & 255;
    let t149 = 1i32.wrapping_add(t140);
    let t327 = t199.wrapping_add(t149);
    let t335 = G3[(P[t327 as usize] & 15) as usize][1];
    let t736 = t103 * t335;
    let t55 = t15 - t47;
    let t119 = t55 - 1.0;
    let t339 = G3[(P[t327 as usize] & 15) as usize][2];
    let t788 = t119 * t339;
    let t792 = t736 + t788;
    let t107 = t27 - t95;
    let t115 = t107 - 1.0;
    let t331 = G3[(P[t327 as usize] & 15) as usize][0];
    let t796 = t115 * t331;
    let t800 = t792 + t796;
    let t153 = P[t128 as usize];
    let t184 = t153.wrapping_add(t144);
    let t195 = P[t184 as usize];
    let t311 = t195.wrapping_add(t149);
    let t319 = G3[(P[t311 as usize] & 15) as usize][1];
    let t581 = t103 * t319;
    let t323 = G3[(P[t311 as usize] & 15) as usize][2];
    let t564 = t119 * t323;
    let t585 = t581 + t564;
    let t315 = G3[(P[t311 as usize] & 15) as usize][0];
    let t572 = t107 * t315;
    let t594 = t585 + t572;
    let t804 = t800 - t594;
    let t176 = t165.wrapping_add(t136);
    let t181 = P[t176 as usize];
    let t295 = t181.wrapping_add(t149);
    let t303 = G3[(P[t295 as usize] & 15) as usize][1];
    let t431 = t83 * t303;
    let t307 = G3[(P[t295 as usize] & 15) as usize][2];
    let t531 = t119 * t307;
    let t547 = t431 + t531;
    let t299 = G3[(P[t295 as usize] & 15) as usize][0];
    let t543 = t115 * t299;
    let t551 = t547 + t543;
    let t812 = t804 - t551;
    let t161 = t153.wrapping_add(t136);
    let t173 = P[t161 as usize];
    let t279 = t173.wrapping_add(t149);
    let t287 = G3[(P[t279 as usize] & 15) as usize][1];
    let t459 = t83 * t287;
    let t291 = G3[(P[t279 as usize] & 15) as usize][2];
    let t507 = t119 * t291;
    let t519 = t459 + t507;
    let t283 = G3[(P[t279 as usize] & 15) as usize][0];
    let t523 = t107 * t283;
    let t535 = t519 + t523;
    let t816 = t812 + t535;
    let t263 = t199.wrapping_add(t140);
    let t271 = G3[(P[t263 as usize] & 15) as usize][1];
    let t411 = t103 * t271;
    let t275 = G3[(P[t263 as usize] & 15) as usize][2];
    let t475 = t55 * t275;
    let t499 = t411 + t475;
    let t267 = G3[(P[t263 as usize] & 15) as usize][0];
    let t495 = t115 * t267;
    let t511 = t499 + t495;
    let t820 = t816 - t511;
    let t243 = t195.wrapping_add(t140);
    let t251 = G3[(P[t243 as usize] & 15) as usize][1];
    let t403 = t103 * t251;
    let t255 = G3[(P[t243 as usize] & 15) as usize][2];
    let t447 = t55 * t255;
    let t463 = t403 + t447;
    let t247 = G3[(P[t243 as usize] & 15) as usize][0];
    let t467 = t107 * t247;
    let t483 = t463 + t467;
    let t824 = t820 + t483;
    let t223 = t181.wrapping_add(t140);
    let t231 = G3[(P[t223 as usize] & 15) as usize][1];
    let t375 = t83 * t231;
    let t235 = G3[(P[t223 as usize] & 15) as usize][2];
    let t395 = t55 * t235;
    let t423 = t375 + t395;
    let t227 = G3[(P[t223 as usize] & 15) as usize][0];
    let t439 = t115 * t227;
    let t455 = t423 + t439;
    let t828 = t824 + t455;
    let t201 = t173.wrapping_add(t140);
    let t211 = G3[(P[t201 as usize] & 15) as usize][1];
    let t351 = t83 * t211;
    let t215 = G3[(P[t201 as usize] & 15) as usize][2];
    let t359 = t55 * t215;
    let t383 = t351 + t359;
    let t207 = G3[(P[t201 as usize] & 15) as usize][0];
    let t367 = t107 * t207;
    let t415 = t383 + t367;
    let t832 = t828 - t415;
    let t1076 = t696 * t832;
    let t760 = t551 - t535;
    let t764 = t760 - t455;
    let t768 = t764 + t415;
    let t1124 = t1076 + t768;
    let t716 = t55 * t55;
    let t724 = t716 * t55;
    let t700 = t55 * 6.0;
    let t708 = t700 + -15.0;
    let t712 = t55 * t708;
    let t720 = t712 + 10.0;
    let t732 = t724 * t720;
    let t1144 = t1124 * t732;
    let t748 = t511 - t483;
    let t752 = t748 - t455;
    let t756 = t752 + t415;
    let t1108 = t696 * t756;
    let t740 = t455 - t415;
    let t1116 = t1108 + t740;
    let t1156 = t1144 + t1116;
    let t603 = t107 * 30.0;
    let t611 = t603 * t107;
    let t590 = t107 - 2.0;
    let t598 = t107 * t590;
    let t607 = t598 + 1.0;
    let t615 = t611 * t607;
    let t1164 = t1156 * t615;
    let t656 = t107 * t107;
    let t664 = t656 * t107;
    let t644 = t107 * 6.0;
    let t648 = t644 + -15.0;
    let t652 = t107 * t648;
    let t660 = t652 + 10.0;
    let t668 = t664 * t660;
    let t840 = t331 - t315;
    let t864 = t840 - t299;
    let t940 = t283 + t864;
    let t952 = t940 - t267;
    let t960 = t247 + t952;
    let t968 = t227 + t960;
    let t980 = t968 - t207;
    let t1012 = t668 * t980;
    let t916 = t315 - t283;
    let t928 = t916 - t247;
    let t936 = t207 + t928;
    let t1028 = t1012 + t936;
    let t1048 = t696 * t1028;
    let t896 = t299 - t283;
    let t904 = t896 - t227;
    let t912 = t207 + t904;
    let t1060 = t668 * t912;
    let t876 = t283 - t207;
    let t1068 = t1060 + t876;
    let t1080 = t1048 + t1068;
    let t1088 = t1080 * t732;
    let t836 = t267 - t247;
    let t860 = t836 - t227;
    let t888 = t207 + t860;
    let t976 = t668 * t888;
    let t868 = t247 - t207;
    let t988 = t976 + t868;
    let t1008 = t696 * t988;
    let t808 = t227 - t207;
    let t1000 = t668 * t808;
    let t1016 = t1000 + t207;
    let t1032 = t1008 + t1016;
    let t1096 = t1088 + t1032;
    let t1172 = t1164 + t1096;
    let t1204 = t1172 * f;
    let t1128 = t668 * t832;
    let t772 = t594 - t535;
    let t776 = t772 - t483;
    let t780 = t776 + t415;
    let t1140 = t1128 + t780;
    let t1176 = t1140 * t732;
    let t1132 = t668 * t756;
    let t744 = t483 - t415;
    let t1152 = t1132 + t744;
    let t1180 = t1176 + t1152;
    let t627 = t83 * 30.0;
    let t635 = t627 * t83;
    let t619 = t83 - 2.0;
    let t623 = t83 * t619;
    let t631 = t623 + 1.0;
    let t640 = t635 * t631;
    let t1188 = t1180 * t640;
    let t844 = t335 - t319;
    let t880 = t844 - t303;
    let t948 = t287 + t880;
    let t956 = t948 - t271;
    let t964 = t251 + t956;
    let t972 = t231 + t964;
    let t984 = t972 - t211;
    let t1044 = t668 * t984;
    let t924 = t319 - t287;
    let t932 = t924 - t251;
    let t944 = t211 + t932;
    let t1056 = t1044 + t944;
    let t1064 = t696 * t1056;
    let t900 = t303 - t287;
    let t908 = t900 - t231;
    let t920 = t211 + t908;
    let t992 = t668 * t920;
    let t848 = t287 - t211;
    let t996 = t992 + t848;
    let t1072 = t1064 + t996;
    let t1084 = t1072 * t732;
    let t852 = t271 - t251;
    let t872 = t852 - t231;
    let t892 = t211 + t872;
    let t1004 = t668 * t892;
    let t884 = t251 - t211;
    let t1024 = t1004 + t884;
    let t1040 = t696 * t1024;
    let t856 = t231 - t211;
    let t1020 = t668 * t856;
    let t1036 = t1020 + t211;
    let t1052 = t1040 + t1036;
    let t1092 = t1084 + t1052;
    let t1196 = t1188 + t1092;
    let t1200 = t1196 * f;
    [t1204, t1200]
}

/// One sample of the quality's sampler (0 = 1D, 1 = 2D, 2 = 3D).
fn perlin(quality: i32, c: V3, f: f32) -> [f32; 2] {
    match quality {
        0 => perlin1d(c, f),
        1 => perlin2d(c, f),
        2 => perlin3d(c, f),
        q => unimplemented!("mod-only: NoiseModule quality {q}: the native logs 'Unexpected number of dimensions' and does nothing"),
    }
}

/// AccumulateNoise<D> (180dce280 / 180dce390 / 180dce4a0): the octaves summed with amplitude and frequency steps,
/// over the amplitudes' sum.
fn accumulate(n: &Noise, c: V3, f: f32) -> [f32; 2] {
    let mut s = perlin(n.quality, c, f);
    let (mut freq, mut amp, mut sum) = (f, 1.0f32, 1.0f32);
    for _ in 1..n.octaves {
        freq = freq * n.octave_scale;
        amp = amp * n.octave_multiplier;
        sum = sum + amp;
        let p = perlin(n.quality, c, freq);
        s = [s[0] + p[0] * amp, s[1] + p[1] * amp];
    }
    [s[0] / sum, s[1] / sum]
}

/// CalculateNoise<...> (180dce5b0..180dd0e40): positionOffset from three draws of Rand(randomSeed), times 100 in the
/// job.
pub fn offset(random_seed: u32) -> V3 {
    draws::<3>(random_seed, 0).map(|v| v * 100.0)
}

/// NoiseModule::Update (180ddf870) when the pass also updates the shape: scrollOffset += scrollSpeed(t / length) * dt.
pub fn scroll(n: &Noise, t: f32, dt: f32, offset: f32) -> f32 {
    evaluate(&n.scroll_speed, t, 1.0) * dt + offset
}

/// CalculateNoiseJob<D, octaves, separate> (180dd11f0..180ddaed0) of one particle: the curl of three samples, the
/// strengths, then animatedVelocity, rotation (z unless 3D) and the impulse UpdateSize reads.
#[allow(clippy::too_many_arguments)]
pub fn job(n: &Noise, offset: V3, scroll: f32, seed: u32, alive: f32, dt: f32, position: V3, rotation_3d: bool, anim: &mut V3, rotation: &mut f32, impulse: &mut V3) {
    if n.remap_enabled {
        unimplemented!("mod-only: NoiseModule remap (the job's remap branch, EvaluateThreaded of remapX/Y/Z)");
    }
    let f = maxps(n.frequency, f32::from_bits(0x358637bd));
    let damp = if n.damping { inverse(f) } else { 1.0 };
    let rot_k = f32::from_bits(0x3c0efa35) * dt;
    let t = maxps(alive * f32::from_bits(0x3c23d70a), 0.0);
    let strength = if n.separate {
        let r = draws::<3>(seed, 0x3edcba94);
        [0, 1, 2].map(|i| evaluate(&n.strength[i], t, r[i]) * damp)
    } else {
        [evaluate(&n.strength[0], t, draws::<1>(seed, 0x3edcba94)[0]) * damp; 3]
    };
    let x = position[0] + offset[0];
    let y = position[1] + offset[1];
    let z = position[2] + offset[2];
    let x1 = x + 100.0;
    // The scroll goes on the last axis the sampler reads.
    let mut coords = [[z, y, x], [x1, z, y], [y, x1, z]];
    let d = n.quality.clamp(0, 2) as usize;
    for c in &mut coords {
        c[d] = c[d] + scroll;
    }
    let [s1, s2, s3] = coords.map(|c| if n.octaves < 2 { perlin(n.quality, c, f) } else { accumulate(n, c, f) });
    let curl = [s3[0] - s2[1], s1[0] - s3[1], s2[0] - s1[1]];
    let v = [0, 1, 2].map(|i| curl[i] * strength[i]);
    let amount = evaluate(&n.amounts[0], t, draws::<1>(seed, 0xb77ce39a)[0]);
    for i in 0..3 {
        anim[i] = anim[i] + v[i] * amount;
    }
    if n.amounts[1].scalar != 0.0 {
        let k = rot_k * evaluate(&n.amounts[1], t, draws::<1>(seed, 0xa92bedcb)[0]);
        if rotation_3d {
            unimplemented!("mod-only: NoiseModule rotation on a 3D rotation: the particles keep z only");
        }
        *rotation = *rotation + k * v[2];
    }
    *impulse = v;
}

/// usesNoiseImpulse (AllocateParticleArrays 1801b4e70): a positive sizeAmount scalar (or the renderer's noise vertex
/// streams, which the rewrite's renderer does not have).
pub fn uses_impulse(n: &Noise) -> bool {
    0.0 < n.amounts[2].scalar
}

/// NoiseModule::UpdateSize (180ddfac0, UpdateSizeTpl<mode> 180ddcf80..180ddd6f0) of one axis, after the size modules:
/// max(0, 0.5 * sizeAmount * impulse + 1) times the size so far; sizeAmount at max(alive * 0.01, 0) with the first draw
/// of seed + 0x38683aaf (the same for every axis).
pub fn size(n: &Noise, seed: u32, alive: f32, impulse: f32, from: f32) -> f32 {
    let t = maxps(alive * f32::from_bits(0x3c23d70a), 0.0);
    let a = evaluate(&n.amounts[2], t, draws::<1>(seed, 0x38683aaf)[0]);
    maxps(0.0, 0.5 * a * impulse + 1.0) * from
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The samplers against the Perlin derivative maths in f64 (a check on the translation, not bit-exact).
    #[test]
    fn samplers_are_the_noise_derivatives() {
        let fade = |t: f64| t * t * t * (t * (t * 6.0 - 15.0) + 10.0);
        let dfade = |t: f64| 30.0 * t * t * (t * (t - 2.0) + 1.0);
        let l = |a: f64, b: f64, t: f64| a + (b - a) * t;
        for &(x, y, z, f) in &[(0.3f32, 1.7f32, -2.2f32, 1.0f32), (12.5, -3.25, 7.75, 0.5), (-0.6, 0.1, 0.9, 2.0)] {
            let (xs, ys, zs) = ((x * f) as f64, (y * f) as f64, (z * f) as f64);
            let (ix, iy, iz) = ((xs.floor() as i64 & 255) as usize, (ys.floor() as i64 & 255) as usize, (zs.floor() as i64 & 255) as usize);
            let (tx, ty, tz) = (xs - xs.floor(), ys - ys.floor(), zs - zs.floor());
            let (ux, uy, uz) = (fade(tx), fade(ty), fade(tz));
            // 1D: d/dx of lerp(g0 t, g1 (t - 1), fade(t)), times 2f.
            let g0 = G1[(P[ix] & 1) as usize] as f64;
            let g1 = G1[(P[ix + 1] & 1) as usize] as f64;
            let d1 = l(g0, g1, ux) + (g1 * (tx - 1.0) - g0 * tx) * dfade(tx);
            let got = perlin1d([x, y, z], f);
            assert!((got[0] as f64 - d1 * 2.0 * f as f64).abs() < 1e-4 && got[1] == 0.0, "1D {got:?} {d1}");
            // 2D: the x and y derivatives, times sqrt(2) f.
            let g = |h: i32| [G2[(h & 7) as usize * 2] as f64, G2[(h & 7) as usize * 2 + 1] as f64];
            let (a, b) = (P[ix] as usize, P[ix + 1] as usize);
            let gs = [[g(P[a + iy]), g(P[a + iy + 1])], [g(P[b + iy]), g(P[b + iy + 1])]];
            let v2 = |i: usize, j: usize| gs[i][j][0] * (tx - i as f64) + gs[i][j][1] * (ty - j as f64);
            let bi = |c: usize| l(l(gs[0][0][c], gs[1][0][c], ux), l(gs[0][1][c], gs[1][1][c], ux), uy);
            let dx = bi(0) + dfade(tx) * l(v2(1, 0) - v2(0, 0), v2(1, 1) - v2(0, 1), uy);
            let dy = bi(1) + dfade(ty) * l(v2(0, 1) - v2(0, 0), v2(1, 1) - v2(1, 0), ux);
            let got = perlin2d([x, y, z], f);
            let s = 2f64.sqrt() * f as f64;
            assert!((got[0] as f64 - dx * s).abs() < 1e-4 && (got[1] as f64 - dy * s).abs() < 1e-4, "2D {got:?} {dx} {dy}");
            // 3D: the x and y derivatives, times f.
            let h = |i: usize, j: usize, k: usize| {
                let gi = P[P[ix + i] as usize + iy + j] + (iz + k) as i32;
                G3[(P[gi as usize] & 15) as usize].map(|v| v as f64)
            };
            let v3 = |i: usize, j: usize, k: usize| {
                let g = h(i, j, k);
                g[0] * (tx - i as f64) + g[1] * (ty - j as f64) + g[2] * (tz - k as f64)
            };
            let tri = |c: usize| {
                let e = |i, j, k| h(i, j, k)[c];
                l(l(l(e(0, 0, 0), e(1, 0, 0), ux), l(e(0, 1, 0), e(1, 1, 0), ux), uy), l(l(e(0, 0, 1), e(1, 0, 1), ux), l(e(0, 1, 1), e(1, 1, 1), ux), uy), uz)
            };
            let dx = tri(0) + dfade(tx) * l(l(v3(1, 0, 0) - v3(0, 0, 0), v3(1, 1, 0) - v3(0, 1, 0), uy), l(v3(1, 0, 1) - v3(0, 0, 1), v3(1, 1, 1) - v3(0, 1, 1), uy), uz);
            let dy = tri(1) + dfade(ty) * l(l(v3(0, 1, 0) - v3(0, 0, 0), v3(1, 1, 0) - v3(1, 0, 0), ux), l(v3(0, 1, 1) - v3(0, 0, 1), v3(1, 1, 1) - v3(1, 0, 1), ux), uz);
            let got = perlin3d([x, y, z], f);
            assert!((got[0] as f64 - dx * f as f64).abs() < 1e-4 && (got[1] as f64 - dy * f as f64).abs() < 1e-4, "3D {got:?} {dx} {dy}");
        }
    }
}
