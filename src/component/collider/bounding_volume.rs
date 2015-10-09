use std::cell::RefCell;
use std::slice::Iter;
use std::iter::Zip;

use math::*;
use stopwatch::Stopwatch;

use component::{TransformManager, EntityMap, EntitySet};
use scene::*;
use ecs::*;
use super::{CachedCollider, ColliderManager, Sphere};
use debug_draw;

// TODO: Build a custom BVH manager that automatically constructs hierarchy.
/// A default manager for component types that can be represented as a single struct.
#[derive(Debug, Clone)]
pub struct BoundingVolumeManager {
    components: Vec<BoundVolume>,
    entities: Vec<Entity>,
    indices: EntityMap<usize>,

    marked_for_destroy: RefCell<EntitySet>,
}

impl BoundingVolumeManager {
    pub fn new() -> BoundingVolumeManager {
        BoundingVolumeManager {
            components: Vec::new(),
            entities: Vec::new(),
            indices: EntityMap::default(),

            marked_for_destroy: RefCell::new(EntitySet::default()),
        }
    }

    pub fn assign(&mut self, entity: Entity, component: BoundVolume) -> &mut BoundVolume {
        assert!(!self.indices.contains_key(&entity));

        let index = self.components.len();
        self.components.push(component);
        self.entities.push(entity);
        self.indices.insert(entity, index);

        &mut self.components[index]
    }

    pub fn get(&self, entity: Entity) -> Option<&BoundVolume> {
        if let Some(index) = self.indices.get(&entity) {
            Some(&self.components[*index])
        } else {
            None
        }
    }

    pub fn get_mut(&mut self, entity: Entity) -> Option<&mut BoundVolume> {
        if let Some(index) = self.indices.get(&entity) {
            Some(&mut self.components[*index])
        } else {
            None
        }
    }

    pub fn components(&self) -> &Vec<BoundVolume> {
        &self.components
    }

    pub fn entities(&self) -> &Vec<Entity> {
        &self.entities
    }

    pub fn iter(&self) -> Zip<Iter<BoundVolume>, Iter<Entity>> {
        self.components.iter().zip(self.entities.iter())
    }

    pub fn destroy_immediate(&mut self, entity: Entity) -> BoundVolume {
        // Retrieve indices of removed entity and the one it's swapped with.
        let index = self.indices.remove(&entity).unwrap();

        // Remove transform and the associate entity.
        let removed_entity = self.entities.swap_remove(index);
        debug_assert!(removed_entity == entity);

        // Update the index mapping for the moved entity, but only if the one we removed
        // wasn't the only one in the row (or the last one in the row).
        if index != self.entities.len() {
            let moved_entity = self.entities[index];
            self.indices.insert(moved_entity, index);
        }

        // Defer removing the transform until the very end to avoid a bunch of memcpys.
        // Transform is a pretty fat struct so if we remove it, cache it to a variable,
        // and then return it at the end we wind up with 2 or 3 memcpys. Doing it all at
        // once at the end (hopefully) means only a single memcpy.
        self.components.swap_remove(index)
    }
}

impl ComponentManager for BoundingVolumeManager {
    fn destroy_all(&self, entity: Entity) {
        if self.indices.contains_key(&entity) {
            self.marked_for_destroy.borrow_mut().insert(entity);
        }
    }

    fn destroy_marked(&mut self) {
        let mut marked_for_destroy = RefCell::new(EntitySet::default());
        ::std::mem::swap(&mut marked_for_destroy, &mut self.marked_for_destroy);
        let mut marked_for_destroy = marked_for_destroy.into_inner();
        for entity in marked_for_destroy.drain() {
            self.destroy_immediate(entity);
        }
    }
}

#[derive(Debug, Clone)]
pub struct BoundVolume {
    pub entity: Entity,
    pub aabb: AABB,
    pub collider: CachedCollider,
}

impl BoundVolume {
    /// Tests if `other` collides with this BVH.
    pub fn test(&self, other: &BoundVolume) -> bool {
        self.aabb.test_aabb(&other.aabb);
        self.collider.test(&other.collider)
    }

    pub fn debug_draw(&self) {
        debug_draw::box_min_max(self.aabb.min, self.aabb.max);
        self.collider.debug_draw();
    }
}

#[derive(Debug, Clone, Copy)]
pub struct AABB {
    pub min: Point,
    pub max: Point,
}

impl AABB {
    /// Given a cached collider generate an AABB that bounds it.
    pub fn from_collider(cached_collider: &CachedCollider) -> AABB {
        match cached_collider {
            &CachedCollider::Sphere(Sphere { center, radius }) => {
                let half_width = Vector3::new(radius, radius, radius);
                let min = center - half_width;
                let max = center + half_width;

                AABB {
                    min: min,
                    max: max,
                }
            },
            &CachedCollider::Box(_) => unimplemented!(),
            &CachedCollider::Mesh => unimplemented!(),
        }
    }

    pub fn test_aabb(&self, other: &AABB) -> bool {
        test_ranges((self.min.x, self.max.x), (other.min.x, other.max.x))
     && test_ranges((self.min.y, self.max.y), (other.min.y, other.max.y))
     && test_ranges((self.min.z, self.max.z), (other.min.z, other.max.z))
    }
}

pub fn bvh_update(scene: &Scene, _delta: f32) {
    let _stopwatch = Stopwatch::new("BVH Update");

    let collider_manager = scene.get_manager::<ColliderManager>();
    let transform_manager = scene.get_manager::<TransformManager>();
    let mut bvh_manager = scene.get_manager_mut::<BoundingVolumeManager>();

    for (entity, collider) in collider_manager.iter() {
        let transform = transform_manager.get(entity);

        let cached_collider = CachedCollider::from_collider_transform(&*collider, &*transform);
        let aabb = AABB::from_collider(&cached_collider);

        // TODO: We can avoid branching here if we create the BVH when the collider is created,
        // or at least do something to ensure that they already exist by the time we get here.
        if let Some(mut bvh) = bvh_manager.get_mut(entity) {
            bvh.collider = cached_collider;
            bvh.aabb = aabb;
            bvh.debug_draw();
            continue;
        }
        // else
        {
            bvh_manager.assign(entity, BoundVolume {
                entity: entity,
                aabb: aabb,
                collider: cached_collider,
            });
        }
    }
}

fn test_ranges(first: (f32, f32), second: (f32, f32)) -> bool {
    let (min_a, max_a) = first;
    let (min_b, max_b) = second;

    !( min_a > max_b
    || min_b > max_a
    || max_a < min_b
    || max_b < min_a)
}