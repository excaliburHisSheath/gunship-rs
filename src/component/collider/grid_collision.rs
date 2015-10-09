use std::collections::{HashMap, HashSet};

use hash::*;
use math::*;
use stopwatch::Stopwatch;

use scene::Scene;
use ecs::Entity;
use super::bounding_volume::*;
use debug_draw;

/// A collision processor that partitions the space into a regular grid.
///
/// # TODO
///
/// - Do something to configure the size of the grid.
#[derive(Debug)]
#[allow(raw_pointer_derive)]
pub struct GridCollisionSystem {
    pub grid: HashMap<GridCell, Vec<(Entity, *const BoundVolume)>, FnvHashState>,
    pub collisions: HashSet<(Entity, Entity), FnvHashState>,
    pub cell_size: f32,
}

impl Clone for GridCollisionSystem {
    fn clone(&self) -> Self {
        GridCollisionSystem::new()
    }
}

impl GridCollisionSystem {
    pub fn new() -> GridCollisionSystem {
        GridCollisionSystem {
            grid: HashMap::default(),
            collisions: HashSet::default(),
            cell_size: 1.0,
        }
    }

    pub fn update(&mut self, scene: &Scene, _delta: f32) {
        let _stopwatch = Stopwatch::new("Grid Collision System");

        // Debug draw the grid.
        for i in -50..50 {
            let offset = i as f32;
            debug_draw::line(
                Point::new(offset * self.cell_size, -50.0 * self.cell_size, 0.0),
                Point::new(offset * self.cell_size,  50.0 * self.cell_size, 0.0));
            debug_draw::line(
                Point::new(-50.0 * self.cell_size, offset * self.cell_size, 0.0),
                Point::new( 50.0 * self.cell_size, offset * self.cell_size, 0.0));
        }

        self.collisions.clear();
        let bvh_manager = scene.get_manager::<BoundingVolumeManager>();

        for bvh in bvh_manager.components() {
            let entity = bvh.entity;

            // Retrieve the AABB at the root of the BVH.
            let aabb = bvh.aabb;

            let min_cell = self.world_to_grid(aabb.min);
            let max_cell = self.world_to_grid(aabb.max);

            // Iterate over all grid cells that the AABB touches. Test the BVH against any entities
            // that have already been placed in that cell, then add the BVH to the cell, creating
            // new cells as necessary.
            for test_cell in min_cell.iter_to(max_cell) {


                if let Some(mut cell) = self.grid.get_mut(&test_cell) {
                    // Check against other volumes.
                    for (other_entity, other_bvh) in cell.iter().cloned() {
                        let other_bvh = unsafe { &*other_bvh };
                        let collision_pair = (entity, other_entity);

                        if bvh.test(other_bvh) {
                            // Woo, we have a collison.
                            self.collisions.insert(collision_pair);
                        }
                    }

                    // Add to existing cell.
                    cell.push((entity, bvh));
                    continue;
                }
                // else
                {
                    let cell = vec![(entity, bvh as *const _)];
                    self.grid.insert(test_cell, cell);
                }
            }
        }

        // Clear out grid contents from previous frame, start each frame with an empty grid an
        // rebuilt it rather than trying to update the grid as objects move.
        for (_, mut cell) in &mut self.grid {
            cell.clear();
        }
    }

    /// Converts a point in world space to its grid cell.
    fn world_to_grid(&self, point: Point) -> GridCell {
        GridCell {
            x: (point.x / self.cell_size).floor() as isize,
            y: (point.y / self.cell_size).floor() as isize,
            z: (point.z / self.cell_size).floor() as isize,
        }
    }
}

/// A wrapper type around a triple of coordinates that uniquely identify a grid cell.
///
/// # Details
///
/// Grid cells are axis-aligned cubes of a regular sice. The coordinates of a grid cell are its min
/// value. This was chosen because of how it simplifies the calculation to find the cell for a
/// given point (`(point / cell_size).floor()`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GridCell {
    pub x: isize,
    pub y: isize,
    pub z: isize,
}

impl GridCell {
    pub fn new(x: isize, y: isize, z: isize) -> GridCell {
        GridCell {
            x: x,
            y: y,
            z: z,
        }
    }

    pub fn iter_to(&self, dest: GridCell) -> GridIter {
        // assert!(self < dest, "start point for grid iter must be less that end point, or use iter_from()");

        GridIter {
            from: *self,
            to:   dest,
            next: *self,
        }
    }
}

pub struct GridIter {
    from: GridCell,
    to:   GridCell,
    next: GridCell,
}

impl Iterator for GridIter {
    type Item = GridCell;

    fn next(&mut self) -> Option<GridCell> {
        let from = self.from;
        let to = self.to;
        let mut next = self.next;

        if next.z >= to.z {
            next.z = from.z;
            if next.y >= to.y {
                next.y = from.y;
                if next.x >= to.x {
                    return None;
                } else {
                    next.x += 1;
                }
            } else {
                next.y += 1;
            }
        } else {
            next.z += 1;
        }

        ::std::mem::swap(&mut self.next, &mut next);
        Some(next)
    }
}