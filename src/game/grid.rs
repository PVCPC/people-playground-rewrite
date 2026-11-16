//! Grid<T> (a width x height array in row order) and the PressureGridController that runs a Grid<PressureCell>: a damped
//! spring on each cell's density. The controller's Dots drawing (Awake's dots, RenderGrid, LateUpdate) is rendering.
#![allow(non_snake_case)]

use super::{d, Game};

/// Prev<T>
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Prev<T> {
    pub Current: T,
    pub Previous: T,
}

/// PressureCell
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PressureCell {
    pub Density: Prev<f32>,
    pub DensityVelocity: Prev<f32>,
}

/// Grid<T>: `Flat[width * y + x]`. The out-of-range ArgumentOutOfRangeException is a panic.
#[derive(Clone, Debug, PartialEq)]
pub struct Grid<T> {
    pub Width: i32,
    pub Height: i32,
    pub Flat: Vec<T>,
}

/// Default: a null Grid (an empty one).
impl<T> Default for Grid<T> {
    fn default() -> Self {
        Grid { Width: 0, Height: 0, Flat: Vec::new() }
    }
}

impl<T: Clone + Default + PartialEq> Grid<T> {
    /// Grid`1..ctor(width, height): `new T[width * height]`.
    pub fn new(width: i32, height: i32) -> Self {
        Grid { Width: width, Height: height, Flat: vec![T::default(); (width * height) as usize] }
    }

    /// Grid`1..ctor(width, height, flat)
    pub fn from_flat(width: i32, height: i32, flat: Vec<T>) -> Self {
        if flat.len() as i32 != width * height {
            panic!("{}", crate::il!(str "Grid`1::.ctor" 0));
        }
        Grid { Width: width, Height: height, Flat: flat }
    }

    /// Grid`1.Get
    pub fn get(&self, x: i32, y: i32) -> &T {
        &self.Flat[self.get_index_from(x, y) as usize]
    }

    /// Grid`1.Get as the reference a class element is.
    pub fn get_mut(&mut self, x: i32, y: i32) -> &mut T {
        let i = self.get_index_from(x, y) as usize;
        &mut self.Flat[i]
    }

    /// Grid`1.TryGet
    pub fn try_get(&self, x: i32, y: i32) -> Option<T> {
        if x < 0 || x >= self.Width || y < 0 || y >= self.Height {
            return None;
        }
        Some(self.Flat[self.get_index_from(x, y) as usize].clone())
    }

    /// Grid`1.Set
    pub fn set(&mut self, x: i32, y: i32, value: T) {
        let i = self.get_index_from(x, y) as usize;
        self.Flat[i] = value;
    }

    /// Grid`1.IEnumerable.GetEnumerator (the non-generic interface): returns GetEnumerator().
    // Grid`1.System.Collections.IEnumerable.GetEnumerator: this fn.
    pub fn enumerable_get_enumerator(&self) -> impl Iterator<Item = (i32, i32, T)> + '_ {
        self.iter()
    }

    /// Grid`1.GetEnumerator: (X, Y, Value), x outer and y inner.
    pub fn iter(&self) -> impl Iterator<Item = (i32, i32, T)> + '_ {
        (0..self.Width).flat_map(move |x| (0..self.Height).map(move |y| (x, y, self.get(x, y).clone())))
    }

    /// Grid`1.BlockEquals
    pub fn block_equals(&self, block: &Grid<T>, top: i32, left: i32) -> bool {
        for (x, y, value) in block.iter() {
            let (num, num2) = (x + left, y + top);
            if num < 0 || num >= self.Width {
                return false;
            }
            if num2 < 0 || num2 >= self.Height {
                return false;
            }
            if *self.get(num, num2) != value {
                return false;
            }
        }
        true
    }

    /// Grid`1.WriteBlock
    pub fn write_block(&mut self, block: &Grid<T>, top: i32, left: i32) {
        for (x, y, value) in block.iter() {
            self.set(x + left, y + top, value);
        }
    }

    /// Grid`1.GetIndexFrom
    pub fn get_index_from(&self, x: i32, y: i32) -> i32 {
        if x < 0 || x >= self.Width {
            panic!("X ({x}) is out of range (0, {})", self.Width - 1);
        }
        if y < 0 || y >= self.Height {
            panic!("Y ({y}) is out of range (0, {})", self.Height - 1);
        }
        Self::index_from_coordinates(x, y, self.Width)
    }

    /// Grid`1.IndexFromCoordinates
    pub fn index_from_coordinates(x: i32, y: i32, w: i32) -> i32 {
        w * y + x
    }

    /// Grid`1.GetAt
    pub fn get_at(flat: &[T], x: i32, y: i32, w: i32) -> T {
        flat[Self::index_from_coordinates(x, y, w) as usize].clone()
    }

    /// Grid`1.SetAt
    pub fn set_at(flat: &mut [T], x: i32, y: i32, w: i32, val: T) {
        flat[Self::index_from_coordinates(x, y, w) as usize] = val;
    }

    /// Grid`1.GetCoordinatesFromIndex (the instance form checks the index against Width * Height).
    pub fn get_coordinates_from_index(&self, index: i32) -> (i32, i32) {
        if index >= self.Width * self.Height {
            panic!("Index ({index}) is out of range ({})", self.Width * self.Height);
        }
        Self::coordinates_from_index(index, self.Width)
    }

    /// Grid`1.GetCoordinatesFromIndex(index, w)
    pub fn coordinates_from_index(index: i32, w: i32) -> (i32, i32) {
        (index % w, index / w)
    }
}

/// PressureGridController
#[derive(Clone, Debug)]
pub struct PressureGrid {
    pub id: i32,
    pub Width: i32,
    pub Height: i32,
    pub GridScale: f32,
    pub Retention: f32,
    pub BalanceForce: f32,
    pub BrushDistance: f32,
    pub Grid: Grid<PressureCell>,
}

impl Default for PressureGrid {
    fn default() -> Self {
        Self {
            id: Default::default(),
            Width: 64,
            Height: 64,
            GridScale: 10.0,
            Retention: 0.8,
            BalanceForce: 0.6,
            BrushDistance: 5.0,
            Grid: Default::default(),
        }
    }
}

impl PressureGrid {
    /// PressureGridController..ctor: Width 64, Height 64, GridScale 10, Retention 0.8, BalanceForce 0.6, BrushDistance 5; the
    /// grid Awake makes (every cell a fresh PressureCell).
    pub fn new(id: i32) -> Self {
        let mut p = PressureGrid { id, Width: 64, Height: 64, GridScale: 10.0, Retention: 0.8, BalanceForce: 0.6, BrushDistance: 5.0, Grid: Grid::new(0, 0) };
        p.make_grid();
        p
    }

    /// The Grid part of PressureGridController.Awake, after the serialised Width and Height are set.
    pub fn make_grid(&mut self) {
        self.Grid = Grid::new(self.Width, self.Height);
    }

    /// PressureGridController.GetWorldPosition
    pub fn get_world_position(&self, x: i32, y: i32) -> [f32; 2] {
        [x as f32 * self.GridScale, y as f32 * self.GridScale]
    }

    /// PressureGridController.FixedUpdate; `brush` is Input.GetKey(KeyCode.Backslash), `mouse` Global.main.MousePosition.
    pub fn fixed_update(&mut self, brush: bool, mouse: [f32; 2]) {
        if brush {
            for i in 0..self.Grid.Width {
                for j in 0..self.Grid.Height {
                    let p = self.get_world_position(i, j);
                    // Vector2.Distance: (float)Math.Sqrt(x * x + y * y) of the difference.
                    let (dx, dy) = (p[0] - mouse[0], p[1] - mouse[1]);
                    if ((d(dx) * dx + d(dy) * dy).0.sqrt() as f32) < self.BrushDistance {
                        self.Grid.get_mut(i, j).Density.Current = 1.0;
                    }
                }
            }
        }
        for k in 0..self.Grid.Width {
            for l in 0..self.Grid.Height {
                let (retention, balance) = (self.Retention, self.BalanceForce);
                let cell = self.Grid.get_mut(k, l);
                let mut previous = cell.DensityVelocity.Previous;
                previous *= retention;
                previous = (d(previous) + (d(0.0) - d(cell.Density.Previous) * balance)).f();
                cell.DensityVelocity.Current = previous;
                cell.Density.Current += previous;
            }
        }
        for m in 0..self.Grid.Width {
            for n in 0..self.Grid.Height {
                let cell = self.Grid.get_mut(m, n);
                cell.Density.Previous = cell.Density.Current;
            }
        }
    }
}

impl Game {
    /// PressureGridController.FixedUpdate of `Game::pressure_grids[i]`.
    pub fn pressure_grid_fixed_update(&mut self, i: usize) {
        let brush = self.input.key(92).held;
        let m = self.MousePosition;
        self.pressure_grids[i].fixed_update(brush, [m[0], m[1]]);
    }
}

/// RadiationGridController.Cell
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RadiationCell {
    pub PreviousValue: f32,
    pub Value: f32,
    pub Velocity: f32,
}

/// RadiationGridController: a damped wave over a dictionary of cells keyed by Vector2Int. The visualisation fields
/// (VisualisationGradient, VisualisationMin, VisualisationMax) are read by nothing. Every cell reads only its
/// neighbours' PreviousValue, so the dictionary's enumeration order cannot change a result.
#[derive(Clone, Debug)]
pub struct RadiationGrid {
    pub id: i32,
    pub GridSize: f32,
    pub VelocityRetainment: f32,
    pub ValueDropRate: f32,
    pub ValueTransferRate: f32,
    pub(crate) grid: std::collections::HashMap<(i32, i32), RadiationCell>,
}

impl Default for RadiationGrid {
    fn default() -> Self {
        Self {
            id: Default::default(),
            GridSize: 0.2,
            VelocityRetainment: 0.99,
            ValueDropRate: 0.4,
            ValueTransferRate: 0.3,
            grid: Default::default(),
        }
    }
}

// RadiationGridController.Awake: the empty dictionary `new` makes
impl RadiationGrid {
    /// RadiationGridController..ctor and Awake: GridSize 0.2, VelocityRetainment 0.99, ValueDropRate 0.4,
    /// ValueTransferRate 0.3 and an empty dictionary.
    pub fn new(id: i32) -> Self {
        Self { id, GridSize: 0.2, VelocityRetainment: 0.99, ValueDropRate: 0.4, ValueTransferRate: 0.3, grid: Default::default() }
    }

    /// RadiationGridController.Start: SetCell(j, i, 0) over 64 x 64.
    pub fn start(&mut self) {
        for i in 0..64 {
            for j in 0..64 {
                self.set_cell((j, i), 0.0);
            }
        }
    }

    /// RadiationGridController.UpdateGrid
    pub fn update_grid(&mut self) {
        for cell in self.grid.values_mut() {
            cell.PreviousValue = cell.Value;
        }
        let keys: Vec<(i32, i32)> = self.grid.keys().copied().collect();
        for key in keys {
            let prev = self.grid[&key].PreviousValue;
            let (drop, rate) = (self.ValueDropRate, self.ValueTransferRate);
            // getDelta: (GetCell(x, y) - cell.PreviousValue) * ValueTransferRate, returned as a float.
            let delta = |s: &Self, x: i32, y: i32| ((d(s.get_cell((x, y), 0.0)) - prev) * rate).f();
            let mut v = self.grid[&key].Velocity;
            v = (d(v) + (d(0.0) - prev) * drop).f();
            for (dx, dy) in [(-1, -1), (0, -1), (1, -1), (-1, 0), (1, 0), (-1, 1), (0, 1), (1, 1)] {
                v = (d(v) + delta(self, key.0 + dx, key.1 + dy)).f();
            }
            v *= self.VelocityRetainment;
            let cell = self.grid.get_mut(&key).unwrap();
            cell.Velocity = v;
            cell.Value += v;
        }
    }

    /// RadiationGridController.SetCell
    pub fn set_cell(&mut self, id: (i32, i32), value: f32) {
        let cell = self.grid.entry(id).or_default();
        let old = cell.Value;
        cell.Value = value;
        cell.Velocity = value - old;
    }

    /// RadiationGridController.GetCell: the cell's PreviousValue, else `fallback`.
    pub fn get_cell(&self, id: (i32, i32), fallback: f32) -> f32 {
        self.grid.get(&id).map_or(fallback, |c| c.PreviousValue)
    }

    /// RadiationGridController.WorldToGridPos: (int)(world / GridSize) per axis: Mono divides in double and conv.i4 truncates that (int.MinValue out of range).
    pub fn world_to_grid_pos(&self, world: [f32; 2]) -> (i32, i32) {
        (super::temperature::conv_i4(world[0] as f64 / self.GridSize as f64), super::temperature::conv_i4(world[1] as f64 / self.GridSize as f64))
    }
}

impl Game {
    /// RadiationGridController.FixedUpdate of `Game::radiation_grids[i]`: the mouse button 0 sets the cell under the
    /// mouse to 1, then UpdateGrid.
    pub fn radiation_grid_fixed_update(&mut self, i: usize) {
        let held = self.input.key(323).held;
        let m = self.MousePosition;
        let g = &mut self.radiation_grids[i];
        if held {
            let at = g.world_to_grid_pos([m[0], m[1]]);
            g.set_cell(at, 1.0);
        }
        g.update_grid();
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn radiation_spike_spreads() {
        let mut g = super::RadiationGrid::new(0);
        g.start();
        g.set_cell((5, 5), 1.0);
        g.update_grid();
        assert_eq!(g.get_cell((5, 5), 0.0), 1.0); // PreviousValue: the value the update started from
        g.update_grid();
        assert!(g.get_cell((4, 5), 0.0) > 0.0);
    }

    use super::*;

    #[test]
    fn grid_and_pressure() {
        let mut g: Grid<i32> = Grid::new(3, 2);
        g.set(2, 1, 7);
        assert_eq!(g.Flat[5], 7);
        assert_eq!(g.try_get(3, 0), None);
        assert_eq!(g.get_coordinates_from_index(5), (2, 1));
        let mut b: Grid<i32> = Grid::new(2, 1);
        b.set(1, 0, 7);
        assert!(g.block_equals(&b, 1, 1));
        g.write_block(&b, 0, 0);
        assert_eq!(*g.get(1, 0), 7);
        let xs: Vec<_> = b.iter().map(|(x, y, _)| (x, y)).collect();
        assert_eq!(xs, vec![(0, 0), (1, 0)]);
        let mut p = PressureGrid::new(1);
        p.fixed_update(true, [0.0, 0.0]);
        assert_eq!(p.Grid.get(0, 0).Density.Previous, 1.0);
        assert_eq!(p.Grid.get(63, 63).Density.Current, 0.0);
    }
}
