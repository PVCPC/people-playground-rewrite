//! LocalFirePropagationMap: fire spreading over a burnable object's serialized burn points, and the Mono
//! Array.Sort its Start shuffles with.
use super::misc::FixedIntervalDistributor;
use super::{d, mathf, Game};

/// LocalFirePropagationMap.BurnPoint
#[derive(Clone, Debug, Default)]
pub struct BurnPoint {
    pub LocalPoint: [f32; 2],
    pub FireIntensity: f32,
    pub Softness: f32,
    pub NeighbourIndices: Vec<i32>,
}

/// LocalFirePropagationMap
#[derive(Clone, Debug)]
pub struct FireMap {
    /// phys (GetComponent<PhysicalBehaviour>), on the same GameObject.
    pub physical: usize,
    /// Collider (its instance id); None when the field is null.
    pub collider: Option<i32>,
    pub Points: Vec<BurnPoint>,
    pub GridStep: f32,
    pub FireIntensityDampening: f32,
    pub FireSpreadSpeed: f32,
    /// CalculationOffset and Randomisation (CalculatePoints).
    pub CalculationOffset: [f32; 2],
    pub Randomisation: f32,
    /// MergeDistanceMultiplier (MergeByDistance).
    pub MergeDistanceMultiplier: f32,
    /// Points came from CalculatePoints, which leaves every BurnPoint.NeighbourIndices null: FixedUpdate throws on the first one.
    pub neighbours_null: bool,
    pub RandomisedIndices: Vec<i32>,
    /// PhysicalBehaviour.fireEmissionInterval (MaxCycles 2); only the fire map's emission uses it.
    pub fireEmissionInterval: FixedIntervalDistributor,
    /// Loaded from a recording whose Start had run: a loaded clone's queued Start leaves the recorded state.
    pub started: bool,
}

impl Default for FireMap {
    fn default() -> Self {
        Self {
            physical: Default::default(),
            collider: Default::default(),
            Points: Default::default(),
            GridStep: 0.2,
            FireIntensityDampening: 3.0,
            FireSpreadSpeed: 1.0,
            CalculationOffset: Default::default(),
            Randomisation: 0.4,
            MergeDistanceMultiplier: 0.6,
            neighbours_null: Default::default(),
            RandomisedIndices: Default::default(),
            fireEmissionInterval: Default::default(),
            started: false,
        }
    }
}

impl FireMap {
    /// LocalFirePropagationMap.MergeByDistance
    pub fn merge_by_distance(&mut self) {
        // float num = MergeDistanceMultiplier * GridStep * GridStep: one double chain, rounded at the local.
        let num = (d(self.MergeDistanceMultiplier) * self.GridStep * self.GridStep).f();
        let n = self.Points.len();
        let mut kept = Vec::with_capacity(n);
        for i in 0..n {
            let mut flag = true;
            for j in i + 1..n {
                let (a, b) = (self.Points[i].LocalPoint, self.Points[j].LocalPoint);
                if mathf::sqr_magnitude([a[0] - b[0], a[1] - b[1]]) < num {
                    flag = false;
                    break;
                }
            }
            if flag {
                kept.push(self.Points[i].clone());
            }
        }
        self.Points = kept;
    }

    /// LocalFirePropagationMap.CalculateNeighbours
    pub fn calculate_neighbours(&mut self) {
        let n = self.Points.len();
        // `sqrMagnitude < GridStep * 4f * GridStep`: the right side is a double chain, compared unrounded.
        let limit = d(self.GridStep) * 4.0 * self.GridStep;
        for i in 0..n {
            let mut slots = [-1i32; 3];
            let mut best = [f32::MAX; 3];
            for j in 0..n {
                if j == i {
                    continue;
                }
                let (a, b) = (self.Points[j].LocalPoint, self.Points[i].LocalPoint);
                let sqr = mathf::sqr_magnitude([a[0] - b[0], a[1] - b[1]]);
                for k in 0..3 {
                    if sqr < best[k] && d(sqr) < limit && !slots.contains(&(j as i32)) {
                        slots[k] = j as i32;
                        best[k] = sqr;
                    }
                }
            }
            self.Points[i].NeighbourIndices = slots.into_iter().filter(|&x| x != -1).collect();
        }
        // Every point now has an array: none is null.
        if n > 0 {
            self.neighbours_null = false;
        }
    }

    pub fn new(physical: usize) -> Self {
        Self {
            physical,
            collider: None,
            Points: Vec::new(),
            GridStep: 0.2,
            FireIntensityDampening: 3.0,
            FireSpreadSpeed: 1.0,
            CalculationOffset: [0.0, 0.0],
            Randomisation: 0.4,
            MergeDistanceMultiplier: 0.6,
            neighbours_null: false,
            RandomisedIndices: Vec::new(),
            fireEmissionInterval: FixedIntervalDistributor { MaxCycles: 2, ..FixedIntervalDistributor::with_rate(60.0) },
            started: false,
        }
    }
}

/// Utils.SmoothApproach(float, float, float, float)
pub(crate) fn smooth_approach(past: f32, target: f32, speed: f32, dt: f32) -> f32 {
    let num = dt * speed;
    let exp = (-(num as f64)).exp() as f32;
    let v = (d(past) - target) * exp + d(target);
    super::physical::nan_fallback(v.f(), 0.0)
}

impl Game {
    /// LocalFirePropagationMap.Awake: phys is the linked `physical`; Points that are not serialized are calculated.
    pub fn fire_map_awake(&mut self, f: usize) {
        if self.fire_maps[f].Points.is_empty() {
            self.fire_map_calculate_points(f);
        }
    }

    /// LocalFirePropagationMap.CalculatePoints: a GridStep grid over Collider.bounds, each node jittered inside a circle of
    /// Randomisation * GridStep, kept when the Collider contains it, as local points. The neighbours stay null.
    pub(crate) fn fire_map_calculate_points(&mut self, f: usize) {
        // LocalFirePropagationMap.StepRepulse: a [Button] of the editor; no code in the game calls it
        let Some(collider) = self.fire_maps[f].collider else { crate::game::menu::throw("NullReferenceException: Object reference not set to an instance of an object") };
        let (p, step, offset, randomisation) = (self.fire_maps[f].physical, self.fire_maps[f].GridStep, self.fire_maps[f].CalculationOffset, self.fire_maps[f].Randomisation);
        // Collider2D.bounds starts with the AutoSyncTransforms.
        self.sync_pending();
        // Bounds.size = extents * 2; Bounds.min = center - extents, Bounds.max = center + extents.
        let bounds = |g: &Game| {
            let (c, e) = g.collider_bounds(p, collider);
            ([c[0] - e[0], c[1] - e[1]], [c[0] + e[0], c[1] + e[1]], [e[0] * 2.0, e[1] * 2.0])
        };
        let (_, _, size) = bounds(self);
        // Mathf.CeilToInt: (int)Math.Ceiling((double)f)
        let capacity = (((size[0] / step) as f64).ceil() as i32) as i64 * (((size[1] / step) as f64).ceil() as i32) as i64;
        let mut points: Vec<BurnPoint> = Vec::new();
        let mut x = bounds(self).0[0];
        while x < bounds(self).1[0] {
            let mut y = bounds(self).0[1];
            while y < bounds(self).1[1] {
                let jitter = self.random.inside_unit_circle();
                let scale = randomisation * step;
                // new Vector2(x, y) + CalculationOffset + (Randomisation * GridStep) * insideUnitCircle
                let vector = [x + offset[0] + scale * jitter[0], y + offset[1] + scale * jitter[1]];
                if self.collider_overlap_point(p, collider, vector) {
                    if points.len() as i64 >= capacity {
                        crate::game::menu::throw("IndexOutOfRangeException: Index was outside the bounds of the array.");
                    }
                    let local = self.inverse_transform_point(p, [vector[0], vector[1], 0.0]);
                    points.push(BurnPoint { LocalPoint: [local[0], local[1]], ..Default::default() });
                }
                y += step;
            }
            x += step;
        }
        self.fire_maps[f].Points = points;
        self.fire_maps[f].neighbours_null = true;
    }

    /// LocalFirePropagationMap.MergeByDistance (an editor [Button]): a point is kept when no later point lies within
    /// MergeDistanceMultiplier * GridStep * GridStep (squared); the kept points in order.
    pub fn fire_map_merge_by_distance(&mut self, f: usize) {
        self.fire_maps[f].merge_by_distance()
    }

    /// LocalFirePropagationMap.CalculateNeighbours (an editor [Button]): each point's up to three nearest others within
    /// (2 * GridStep)^2, by the game's slot-filling loop.
    pub fn fire_map_calculate_neighbours(&mut self, f: usize) {
        self.fire_maps[f].calculate_neighbours()
    }

    /// LocalFirePropagationMap.Start
    pub fn fire_map_start(&mut self, f: usize) {
        if std::mem::take(&mut self.fire_maps[f].started) {
            return;
        }
        let n = self.fire_maps[f].Points.len();
        let mut indices = Vec::with_capacity(n);
        for i in 0..n {
            // 1.28.57: Random.Range(0.004f, 1f); 1.28.41: Random.Range(0.001f, 1f).
            let min = if crate::assets::il::is_1_28_45() { 0.004 } else { 0.001 };
            self.fire_maps[f].Points[i].Softness = self.random.range(min, 1.0);
            indices.push(i as i32);
        }
        // Array.Sort(RandomisedIndices, (a, b) => Random.value > 0f ? 1 : -1)
        let random = &mut self.random;
        introspective_sort(&mut indices, &mut |_, _| if random.value() > 0.0 { 1 } else { -1 });
        self.fire_maps[f].RandomisedIndices = indices;
    }

    /// LocalFirePropagationMap.FixedUpdate
    pub fn fire_map_fixed_update(&mut self, f: usize) {
        let dt = self.time.fixedDeltaTime;
        let on_fire = self.physicals[self.fire_maps[f].physical].OnFire;
        let m = &mut self.fire_maps[f];
        // Points[0].NeighbourIndices.Length of a calculated map: NullReferenceException before anything is written.
        if m.neighbours_null && !m.Points.is_empty() {
            super::menu::throw("NullReferenceException: Object reference not set to an instance of an object");
        }
        for i in 0..m.Points.len() {
            for j in 0..m.Points[i].NeighbourIndices.len() {
                let num = m.Points[i].NeighbourIndices[j] as usize;
                let burn = m.Points[num].FireIntensity;
                if burn > m.Points[i].FireIntensity {
                    let speed = m.FireSpreadSpeed * m.Points[i].Softness;
                    m.Points[i].FireIntensity = smooth_approach(m.Points[i].FireIntensity, burn, speed, dt);
                } else {
                    let speed = m.FireSpreadSpeed * m.Points[num].Softness;
                    m.Points[num].FireIntensity = smooth_approach(burn, m.Points[i].FireIntensity, speed, dt);
                }
            }
            if !on_fire {
                let speed = m.FireIntensityDampening * m.Points[i].Softness;
                m.Points[i].FireIntensity = smooth_approach(m.Points[i].FireIntensity, 0.0, speed, dt);
            }
        }
    }

    /// LocalFirePropagationMap.GetRandomFireyIndex
    pub fn fire_map_random_firey_index(&mut self, f: usize, minimum: f32) -> i32 {
        let len = self.fire_maps[f].Points.len() as i32;
        let mut result = -1;
        let num = 1.0 / len as f32;
        let mut num2 = self.random.range_int(0, len);
        for _ in 0..len {
            num2 = (num2 + 1) % len;
            if self.fire_maps[f].Points[num2 as usize].FireIntensity > minimum {
                result = num2;
            }
            if num > self.random.value() {
                return result;
            }
        }
        result
    }

    /// PhysicalBehaviour.Ignite(bool)'s Points[Random.Range(0, Points.Length)].FireIntensity = 1f.
    pub fn fire_map_ignite_random(&mut self, f: usize) {
        let len = self.fire_maps[f].Points.len() as i32;
        let i = self.random.range_int(0, len);
        let Some(point) = self.fire_maps[f].Points.get_mut(i as usize) else {
            crate::game::menu::throw("IndexOutOfRangeException: Index was outside the bounds of the array.")
        };
        point.FireIntensity = 1.0;
    }

    /// LocalFirePropagationMap.IgniteAt
    pub fn fire_map_ignite_at(&mut self, f: usize, world_point: [f32; 2]) {
        let Some(collider) = self.fire_maps[f].collider else { super::menu::throw("NullReferenceException: Object reference not set to an instance of an object") };
        // Collider2D.ClosestPoint starts with the AutoSyncTransforms.
        self.sync_pending();
        let c = self.world.collider_closest_point(collider, crate::b2::math::Vec2::new(world_point[0], world_point[1]));
        let local = self.inverse_transform_point(self.fire_maps[f].physical, [c.x, c.y, 0.0]);
        let m = &mut self.fire_maps[f];
        let num = m.GridStep * m.GridStep;
        let mut num2 = f32::MAX;
        let mut num3 = 0;
        for i in 0..m.Points.len() {
            let sqr = mathf::sqr_magnitude([m.Points[i].LocalPoint[0] - local[0], m.Points[i].LocalPoint[1] - local[1]]);
            if sqr < num2 {
                num2 = sqr;
                num3 = i;
            }
            if sqr < num {
                m.Points[i].FireIntensity = 1.0;
            }
        }
        let Some(point) = m.Points.get_mut(num3) else { crate::game::menu::throw("IndexOutOfRangeException: Index was outside the bounds of the array.") };
        point.FireIntensity = 1.0;
    }

    /// PhysicalBehaviour.SetParticleEmission's fire-map branch (BurnIntensity > 0.01): the Random draws of the
    /// emission; the emitted particles are presentation.
    pub fn fire_map_emission(&mut self, p: usize, f: usize) {
        let e = self.sprite_world_bounds(p).1;
        let sqr = (d(e[0]) * e[0] + d(e[1]) * e[1] + d(e[2]) * e[2]).f();
        let num = mathf::sqrt(sqr * 4.0);
        let ph = &self.physicals[p];
        let num2 = ((d(1.0) - ph.BurnProgress) * ph.burnIntensity * 2.0f32 * num).f();
        let len = self.fire_maps[f].Points.len() as i32;
        // 1.28.57: RateHz = 12f * (float)Mathf.Max(Points.Length, 25) * 0.2f; 1.28.41: 15f * ... * 0.06f.
        let rate = if crate::assets::il::is_1_28_45() { (d(12.0) * len.max(25) as f32 * 0.2f32).f() } else { (d(15.0) * len.max(25) as f32 * 0.06f32).f() };
        self.fire_maps[f].fireEmissionInterval.IntervalSeconds = 1.0 / rate;
        let dt = self.time.deltaTime;
        // The loop condition re-runs CalculateCycleCount (adding dt again) every iteration.
        let mut i = 0;
        while i < self.fire_maps[f].fireEmissionInterval.calculate_cycle_count(dt) {
            i += 1;
            let index = self.fire_map_random_firey_index(f, 0.05);
            if index != -1 {
                let fi = self.fire_maps[f].Points[index as usize].FireIntensity;
                // 1.28.57: Mathf.Clamp01(fireIntensity * fireIntensity * num2); 1.28.41: fireIntensity^6 * num2.
                let new = crate::assets::il::is_1_28_45();
                let num3 = if new { mathf::clamp01((d(fi) * fi * num2).f()) } else { (d(fi) * fi * fi * fi * fi * fi * num2).f() };
                if self.random.value() < num3 && !self.effects_off {
                    // FireParticleSystem.Emit(position = TransformPoint(LocalPoint), startSize, startColor).
                    let local = self.fire_maps[f].Points[index as usize].LocalPoint;
                    let position = self.transform_point3(p, [local[0], local[1], 0.0]);
                    // startColor = Color.Lerp(Color.black, Color.white, t) (Lerp clamps t): 1.28.57's t is 0.4f + num3.
                    let grey = mathf::clamp01(if new { 0.4 + num3 } else { num3 });
                    // 1.28.57: startSize = Mathf.Min(num3 * 2f, 3.5f) + Mathf.Clamp01(num * 0.25f); 1.28.41: Mathf.Min(num3 * 2f, 2.5f).
                    let size = if new { mathf::min(num3 * 2.0, 3.5) + mathf::clamp01(num * 0.25) } else { mathf::min(num3 * 2.0, 2.5) };
                    self.fire_emits.push((p, position, size, [grey, grey, grey, 1.0]));
                }
            }
        }
    }
}

/// IntrospectiveSortUtilities.FloorLog2
fn floor_log2(mut n: i32) -> i32 {
    let mut num = 0;
    while n >= 1 {
        num += 1;
        n /= 2;
    }
    num
}

/// Mono ArraySortHelper<T>.Sort(keys, 0, keys.Length, Comparison): IntrospectiveSort. An index the comparer drives
/// out of the array is the IndexOutOfRangeException Mono turns into ThrowOrIgnoreBadComparer.
pub fn introspective_sort(keys: &mut [i32], cmp: &mut impl FnMut(i32, i32) -> i32) {
    if keys.len() >= 2 {
        let hi = keys.len() as isize - 1;
        intro_sort(keys, 0, hi, 2 * floor_log2(keys.len() as i32), cmp);
    }
}

fn at(keys: &[i32], i: isize) -> i32 {
    *usize::try_from(i).ok().and_then(|i| keys.get(i)).unwrap_or_else(|| crate::game::menu::throw("IndexOutOfRangeException: Index was outside the bounds of the array."))
}

fn swap_if_greater(keys: &mut [i32], cmp: &mut impl FnMut(i32, i32) -> i32, a: isize, b: isize) {
    if a != b && cmp(at(keys, a), at(keys, b)) > 0 {
        keys.swap(a as usize, b as usize);
    }
}

fn intro_sort(keys: &mut [i32], lo: isize, mut hi: isize, mut depth_limit: i32, cmp: &mut impl FnMut(i32, i32) -> i32) {
    while hi > lo {
        let num = hi - lo + 1;
        if num <= 16 {
            match num {
                1 => {}
                2 => swap_if_greater(keys, cmp, lo, hi),
                3 => {
                    swap_if_greater(keys, cmp, lo, hi - 1);
                    swap_if_greater(keys, cmp, lo, hi);
                    swap_if_greater(keys, cmp, hi - 1, hi);
                }
                _ => insertion_sort(keys, lo, hi, cmp),
            }
            break;
        }
        if depth_limit == 0 {
            heapsort(keys, lo, hi, cmp);
            break;
        }
        depth_limit -= 1;
        let num2 = pick_pivot_and_partition(keys, lo, hi, cmp);
        intro_sort(keys, num2 + 1, hi, depth_limit, cmp);
        hi = num2 - 1;
    }
}

fn pick_pivot_and_partition(keys: &mut [i32], lo: isize, hi: isize, cmp: &mut impl FnMut(i32, i32) -> i32) -> isize {
    let num = lo + (hi - lo) / 2;
    swap_if_greater(keys, cmp, lo, num);
    swap_if_greater(keys, cmp, lo, hi);
    swap_if_greater(keys, cmp, num, hi);
    let val = keys[num as usize];
    keys.swap(num as usize, (hi - 1) as usize);
    let (mut num2, mut num3) = (lo, hi - 1);
    while num2 < num3 {
        loop {
            num2 += 1;
            if cmp(at(keys, num2), val) >= 0 {
                break;
            }
        }
        loop {
            num3 -= 1;
            if cmp(val, at(keys, num3)) >= 0 {
                break;
            }
        }
        if num2 >= num3 {
            break;
        }
        keys.swap(num2 as usize, num3 as usize);
    }
    keys.swap(num2 as usize, (hi - 1) as usize);
    num2
}

fn heapsort(keys: &mut [i32], lo: isize, hi: isize, cmp: &mut impl FnMut(i32, i32) -> i32) {
    let num = hi - lo + 1;
    for i in (1..=num / 2).rev() {
        down_heap(keys, i, num, lo, cmp);
    }
    for i in (2..=num).rev() {
        keys.swap(lo as usize, (lo + i - 1) as usize);
        down_heap(keys, 1, i - 1, lo, cmp);
    }
}

fn down_heap(keys: &mut [i32], mut i: isize, n: isize, lo: isize, cmp: &mut impl FnMut(i32, i32) -> i32) {
    let val = keys[(lo + i - 1) as usize];
    while i <= n / 2 {
        let mut num = 2 * i;
        if num < n && cmp(keys[(lo + num - 1) as usize], keys[(lo + num) as usize]) < 0 {
            num += 1;
        }
        if cmp(val, keys[(lo + num - 1) as usize]) >= 0 {
            break;
        }
        keys[(lo + i - 1) as usize] = keys[(lo + num - 1) as usize];
        i = num;
    }
    keys[(lo + i - 1) as usize] = val;
}

fn insertion_sort(keys: &mut [i32], lo: isize, hi: isize, cmp: &mut impl FnMut(i32, i32) -> i32) {
    for i in lo..hi {
        let mut num = i;
        let val = keys[(i + 1) as usize];
        while num >= lo && cmp(val, keys[num as usize]) < 0 {
            keys[(num + 1) as usize] = keys[num as usize];
            num -= 1;
        }
        keys[(num + 1) as usize] = val;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn introsort_sorts() {
        let mut seed = 12345u32;
        for n in 0..200 {
            let mut keys: Vec<i32> = (0..n)
                .map(|_| {
                    seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                    (seed >> 16) as i32 % 50
                })
                .collect();
            let mut want = keys.clone();
            want.sort();
            introspective_sort(&mut keys, &mut |a, b| a.cmp(&b) as i32);
            assert_eq!(keys, want);
            // The heapsort fallback.
            let mut keys2 = want.iter().rev().copied().collect::<Vec<_>>();
            if n >= 2 {
                heapsort(&mut keys2, 0, n as isize - 1, &mut |a, b| a.cmp(&b) as i32);
            }
            assert_eq!(keys2, want);
        }
    }

    #[test]
    fn always_greater_comparer() {
        // Start's comparer: Random.value > 0 is (almost) always true, so every comparison says "greater".
        for n in 0..100 {
            let mut keys: Vec<i32> = (0..n).collect();
            let mut calls = 0;
            introspective_sort(&mut keys, &mut |_, _| {
                calls += 1;
                1
            });
            let mut sorted = keys.clone();
            sorted.sort();
            assert_eq!(sorted, (0..n).collect::<Vec<_>>());
        }
    }
}
