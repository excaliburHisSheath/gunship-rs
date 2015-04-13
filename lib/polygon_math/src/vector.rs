#[repr(C)] #[derive(Debug, Clone, Copy)]
pub struct Vector3 {
    pub x: f32,
    pub y: f32,
    pub z: f32
}

impl Vector3 {
    pub fn cross(first: Vector3, second: Vector3) -> Vector3 {
        Vector3 {
            x: first.y * second.z - first.z * second.y,
            y: first.z * second.x - first.x * second.z,
            z: first.x * second.y - first.y * second.x
        }
    }

    pub fn normalize(&mut self) {
        let one_over_magnitude = 1.0 / self.magnitude();
        self.x *= one_over_magnitude;
        self.y *= one_over_magnitude;
        self.z *= one_over_magnitude;
    }

    pub fn normalized(&self) -> Vector3 {
        let mut copy = *self;
        copy.normalize();
        copy
    }

    pub fn magnitude(&self) -> f32 {
        self.magnitude_squared().sqrt()
    }

    pub fn magnitude_squared(&self) -> f32 {
        self.x * self.x + self.y * self.y + self.z * self.z
    }
}

pub fn vector3(x: f32, y: f32, z: f32) -> Vector3 {
    Vector3 {
        x: x,
        y: y,
        z: z
    }
}
