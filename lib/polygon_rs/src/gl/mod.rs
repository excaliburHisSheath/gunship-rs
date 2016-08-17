pub extern crate gl_util;

use {Counter, GpuMesh, Renderer};
use anchor::*;
use camera::*;
use geometry::mesh::{Mesh, VertexAttribute};
use light::*;
use material::*;
use mesh_instance::*;
use math::*;
use self::gl_util::{
    AttribLayout,
    Comparison,
    DestFactor,
    DrawBuilder,
    DrawMode,
    Face,
    GlMatrix,
    IndexBuffer,
    Program,
    Shader as GlShader,
    ShaderType,
    SourceFactor,
    VertexBuffer,
};
use self::gl_util::context::{Context, Error as ContextError};
use self::gl_util::texture::{
    Texture2d as GlTexture2d,
    TextureFormat,
    TextureInternalFormat,
};
use shader::Shader;
use std::collections::HashMap;
use std::str;
use texture::*;

static DEFAULT_SHADER_BYTES: &'static [u8] = include_bytes!("../../resources/materials/texture_diffuse_lit.material");

#[derive(Debug)]
pub struct GlRender {
    context: Context,

    materials: HashMap<MaterialId, Material>,
    meshes: HashMap<GpuMesh, MeshData>,
    textures: HashMap<GpuTexture, GlTexture2d>,
    mesh_instances: HashMap<MeshInstanceId, MeshInstance>,
    anchors: HashMap<AnchorId, Anchor>,
    cameras: HashMap<CameraId, Camera>,
    lights: HashMap<LightId, Light>,
    programs: HashMap<Shader, Program>,

    material_counter: MaterialId,
    mesh_counter: GpuMesh,
    texture_counter: GpuTexture,
    mesh_instance_counter: MeshInstanceId,
    anchor_counter: AnchorId,
    camera_counter: CameraId,
    light_counter: LightId,
    shader_counter: Shader,

    default_material: Material,
}

impl GlRender {
    pub fn new() -> Result<GlRender, Error> {
        let context = Context::new()?;

        let mut renderer = GlRender {
            context: context,

            materials: HashMap::new(),
            meshes: HashMap::new(),
            textures: HashMap::new(),
            mesh_instances: HashMap::new(),
            anchors: HashMap::new(),
            cameras: HashMap::new(),
            lights: HashMap::new(),
            programs: HashMap::new(),

            material_counter: MaterialId::initial(),
            mesh_counter: GpuMesh::initial(),
            texture_counter: GpuTexture::initial(),
            mesh_instance_counter: MeshInstanceId::initial(),
            anchor_counter: AnchorId::initial(),
            camera_counter: CameraId::initial(),
            light_counter: LightId::initial(),
            shader_counter: Shader::initial(),

            // Use temporary value and replace it later.
            default_material: Material::new(Shader::initial()),
        };

        // Load source code for the default material.
        let default_material_source = str::from_utf8(DEFAULT_SHADER_BYTES).unwrap();
        let material_source = MaterialSource::from_str(default_material_source).unwrap();

        // default_material.set_color("surface_color", Color::new(0.25, 0.25, 0.25, 1.0));
        // default_material.set_color("surface_specular", Color::new(1.0, 1.0, 1.0, 1.0));
        // default_material.set_f32("surface_shininess", 3.0);

        // Create the default material and drop add it to the renderer.
        let default_material = renderer.build_material(material_source).unwrap();
        renderer.default_material = default_material;

        Ok(renderer)
    }

    fn draw_mesh(
        &self,
        mesh_data: &MeshData,
        material: &Material,
        model_transform: Matrix4,
        normal_transform: Matrix3,
        camera: &Camera,
        camera_anchor: &Anchor,
    ) {
        let default_texture = GlTexture2d::default();

        // Calculate the various transforms needed for rendering.
        let view_transform = camera_anchor.view_matrix();
        let model_view_transform = view_transform * model_transform;
        let projection_transform = camera.projection_matrix();
        let model_view_projection = projection_transform * model_view_transform;

        let view_normal_transform = {
            let inverse_model = normal_transform.transpose();
            let inverse_view = camera_anchor.inverse_view_matrix().into();
            let inverse_model_view = inverse_model * inverse_view;
            inverse_model_view.transpose()
        };

        let program = self
            .programs
            .get(material.shader())
            .expect("Material is using a shader that does not exist");

        // Set the shader to use.
        let mut draw_builder = DrawBuilder::new(&mesh_data.vertex_buffer, DrawMode::Triangles);
        draw_builder
        .index_buffer(&mesh_data.index_buffer)
        .program(program)
        .cull(Face::Back)
        .depth_test(Comparison::Less)

        // Associate vertex attributes with shader program variables.
        .map_attrib_name("position", "vertex_position")
        .map_attrib_name("normal", "vertex_normal")
        .map_attrib_name("texcoord", "vertex_uv0")

        // Set uniform transforms.
        .uniform(
            "model_transform",
            GlMatrix {
                data: model_transform.raw_data(),
                transpose: true,
            })
        .uniform(
            "normal_transform",
            GlMatrix {
                data: normal_transform.raw_data(),
                transpose: true,
            })
        .uniform(
            "view_normal_transform",
            GlMatrix {
                data: view_normal_transform.raw_data(),
                transpose: true,
            })
        .uniform(
            "view_transform",
            GlMatrix {
                data: view_transform.raw_data(),
                transpose: true,
            })
        .uniform(
            "model_view_transform",
            GlMatrix {
                data: model_view_transform.raw_data(),
                transpose: true,
            })
        .uniform(
            "projection_transform",
            GlMatrix {
                data: projection_transform.raw_data(),
                transpose: true,
            })
        .uniform(
            "model_view_projection",
            GlMatrix {
                data: model_view_projection.raw_data(),
                transpose: true,
            })

        // Set uniform colors.
        .uniform("global_ambient", [0.01, 0.01, 0.01, 1.0])

        // Other uniforms.
        .uniform("camera_position", *camera_anchor.position().as_array());

        // Apply material attributes.
        for (name, property) in material.properties() {
            match *property {
                MaterialProperty::Color(ref color) => {
                    draw_builder.uniform::<[f32; 4]>(name, color.into());
                },
                MaterialProperty::f32(value) => {
                    draw_builder.uniform(name, value);
                },
                MaterialProperty::Vector3(value) => {
                    draw_builder.uniform::<[f32; 3]>(name, value.into());
                },
                MaterialProperty::Texture(ref texture) => {
                    let gl_texture =
                        self.textures
                        .get(texture)
                        .unwrap_or(&default_texture);
                    draw_builder.uniform(name, gl_texture);
                },
            }
        }

        // Render first light without blending so it overrides any objects behind it.
        // We also render it with light strength 0 so it only renders ambient color.
        draw_builder
            .uniform("light_position", *Point::origin().as_array())
            .uniform("light_strength", 0.0)
            .draw();

        // Render the rest of the lights with blending on the the depth check set to
        // less than or equal.
        draw_builder
            .depth_test(Comparison::LessThanOrEqual)
            .blend(SourceFactor::One, DestFactor::One);

        for light in self.lights.values() {
            // Send the light's position in view space.
            let light_anchor = match light.anchor() {
                Some(anchor_id) => self.anchors.get(&anchor_id).expect("No such anchor exists"),
                None => panic!("Cannot render light if it's not attached to an anchor"),
            };
            draw_builder.uniform("light_position", *light_anchor.position().as_array());

            let light_position_view = light_anchor.position() * view_transform;
            draw_builder.uniform("light_position_view", *light_position_view.as_array());

            // Send common light data.
            draw_builder.uniform::<[f32; 4]>("light_color", light.color.into());
            draw_builder.uniform("light_strength", light.strength);

            // Send data specific to the current type of light.
            match light.data {
                LightData::Point(PointLight { radius }) => {
                    draw_builder.uniform("light_radius", radius);
                },
            }

            // Draw the current light.
            draw_builder.draw();
        }
    }

    /// Clears the current back buffer.
    pub fn clear(&self) {
        self.context.clear();
    }

    /// Swap the front and back buffers for the render system.
    pub fn swap_buffers(&self) {
        self.context.swap_buffers();
    }
}

impl Drop for GlRender {
    fn drop(&mut self) {
        // Empty all containers to force cleanup of OpenGL primitives before we tear down the
        // GL subsystem.
        self.materials.clear();
        self.meshes.clear();
        self.textures.clear();
        self.mesh_instances.clear();
        self.anchors.clear();
        self.cameras.clear();
        self.lights.clear();
        self.programs.clear();
    }
}

impl Renderer for GlRender {
    fn draw(&mut self) {
        self.clear();

        let (camera, camera_anchor) = if let Some(camera) = self.cameras.values().next() {
            // Use the first camera in the scene for now. Eventually we'll want to support
            // rendering multiple cameras to multiple viewports or render targets but for now one
            // is enough.
            let anchor = match camera.anchor() {
                Some(ref anchor_id) => self.anchors.get(anchor_id).expect("no such anchor exists"),
                None => unimplemented!(),
            };

            (camera, anchor)
        } else {
            panic!("There must be a camera registered");
        };

        for mesh_instance in self.mesh_instances.values() {
            let anchor = match mesh_instance.anchor() {
                Some(anchor_id) => self.anchors.get(anchor_id).expect("No such anchor exists"),
                None => continue,
            };

            let model_transform = anchor.matrix();
            let normal_transform = anchor.normal_matrix();

            let mesh = self.meshes.get(mesh_instance.mesh()).expect("Mesh data does not exist for mesh id");

            self.draw_mesh(
                mesh,
                &mesh_instance.material(),
                model_transform,
                normal_transform,
                camera,
                camera_anchor);
        }

        self.swap_buffers();
    }

    fn default_material(&self) -> Material {
        self.default_material.clone()
    }

    fn build_material(&mut self, source: MaterialSource) -> Result<Material, ()> {
        use polygon_material::material_source::PropertyType;

        // COMPILE SHADER SOURCE
        // =====================

        // Generate uniform declarations for the material's properties. This string will be
        // injected into the shader templates.
        let uniform_declarations = {
            let mut uniform_declarations = String::new();
            for property in &source.properties {
                uniform_declarations.push_str("uniform ");

                let type_str = match property.property_type {
                    PropertyType::Color => "vec4",
                    PropertyType::Texture2d => "sampler2D",
                    PropertyType::f32 => "float",
                    PropertyType::Vector3 => "vec3",
                };

                uniform_declarations.push_str(type_str);
                uniform_declarations.push(' ');
                uniform_declarations.push_str(&*property.name);
                uniform_declarations.push_str(";\n");
            }

            uniform_declarations
        };

        static BUILT_IN_UNIFORMS: &'static str = r#"
            uniform mat4 model_transform;
            uniform mat3 normal_transform;
            uniform mat3 view_normal_transform;
            uniform mat4 view_transform;
            uniform mat4 model_view_transform;
            uniform mat4 projection_transform;
            uniform mat4 model_view_projection;

            uniform vec4 global_ambient;
            uniform vec4 camera_position;
            uniform vec4 camera_position_view;
            uniform vec4 light_position;
            uniform vec4 light_position_view;
            uniform float light_strength;
            uniform float light_radius;
            uniform vec4 light_color;
        "#;

        // Generate the GLSL source for the vertex shader.
        let vert_shader = {
            static DEFAULT_VERT_MAIN: &'static str = r#"
                @position = model_view_projection * vertex_position;

                @vertex.position = vertex_position;
                @vertex.normal = vertex_normal;
                @vertex.uv0 = vertex_uv0;

                @vertex.world_position = model_transform * vertex_position;
                @vertex.world_normal = normalize(normal_transform * vertex_normal);

                @vertex.view_position = model_view_transform * vertex_position;
                @vertex.view_normal = normalize(view_normal_transform * vertex_normal);
            "#;

            // Retrieve source string for the vertex shader.
            let raw_source =
                source
                .programs
                .iter()
                .find(|program_source| program_source.is_vertex())
                .map(|program_source| program_source.source())
                .unwrap_or(DEFAULT_VERT_MAIN);

            // Perform text replacements for the various keywords.
            let replaced_source = raw_source
                .replace("@position", "gl_Position")
                .replace("@vertex.position", "_vertex_position_")
                .replace("@vertex.normal", "_vertex_normal_")
                .replace("@vertex.uv0", "_vertex_uv0_")
                .replace("@vertex.world_position", "_vertex_world_position_")
                .replace("@vertex.world_normal", "_vertex_world_normal_")
                .replace("@vertex.view_position", "_vertex_view_position_")
                .replace("@vertex.view_normal", "_vertex_view_normal_");
            let replaced_source = format!(r#"
                    #version 150

                    {}

                    {}

                    in vec4 vertex_position;
                    in vec3 vertex_normal;
                    in vec2 vertex_uv0;

                    out vec4 _vertex_position_;
                    out vec3 _vertex_normal_;
                    out vec2 _vertex_uv0_;
                    out vec4 _vertex_world_position_;
                    out vec3 _vertex_world_normal_;
                    out vec4 _vertex_view_position_;
                    out vec3 _vertex_view_normal_;

                    void main(void) {{
                        {}
                    }}
                "#,
                BUILT_IN_UNIFORMS,
                uniform_declarations,
                replaced_source);

            GlShader::new(replaced_source, ShaderType::Vertex).map_err(|err| ())?
        };

        // Generate the GLSL source for the fragment shader.
        let frag_shader = {
            // Retrieve source string for the fragment shader.
            let raw_source =
                source
                .programs
                .iter()
                .find(|program_source| program_source.is_fragment())
                .map(|program_source| program_source.source())
                .ok_or(())?;

            // Perform text replacements for the various keywords.
            let replaced_source = raw_source
                .replace("@color", "_fragment_color_")
                .replace("@vertex.position", "_vertex_position_")
                .replace("@vertex.normal", "_vertex_normal_")
                .replace("@vertex.uv0", "_vertex_uv0_")
                .replace("@vertex.world_position", "_vertex_world_position_")
                .replace("@vertex.world_normal", "_vertex_world_normal_")
                .replace("@vertex.view_position", "_vertex_view_position_")
                .replace("@vertex.view_normal", "_vertex_view_normal_");
            let replaced_source = format!(r#"
                    #version 150

                    {}

                    {}

                    in vec4 _vertex_position_;
                    in vec3 _vertex_normal_;
                    in vec2 _vertex_uv0_;
                    in vec4 _vertex_world_position_;
                    in vec3 _vertex_world_normal_;
                    in vec4 _vertex_view_position_;
                    in vec3 _vertex_view_normal_;

                    out vec4 _fragment_color_;

                    void main(void) {{
                        {}
                    }}
                "#,
                BUILT_IN_UNIFORMS,
                uniform_declarations,
                replaced_source);

            GlShader::new(replaced_source, ShaderType::Fragment).map_err(|err| ())?
        };

        let program = Program::new(&[vert_shader, frag_shader]).map_err(|err| ())?;

        let program_id = self.shader_counter.next();
        self.programs.insert(program_id, program);

        // BUILD MATERIAL OBJECT
        // =====================

        let mut material = Material::new(program_id);

        // Add the properties from the material declaration.
        for property in source.properties {
            match property.property_type {
                PropertyType::Color => material.set_color(property.name, Color::default()),
                PropertyType::Texture2d => material.set_texture(property.name, GpuTexture::default()),
                PropertyType::f32 => material.set_f32(property.name, f32::default()),
                PropertyType::Vector3 => material.set_vector3(property.name, Vector3::default()),
            };
        }

        Ok(material)
    }

    fn register_material(&mut self, material: Material) -> MaterialId {
        let material_id = self.material_counter.next();

        let old = self.materials.insert(material_id, material);
        assert!(old.is_none());

        material_id
    }

    fn get_material(&self, material_id: MaterialId) -> Option<&Material> {
        self.materials.get(&material_id)
    }

    fn register_mesh(&mut self, mesh: &Mesh) -> GpuMesh {
        // Generate array buffer.
        let mut vertex_buffer = VertexBuffer::new();
        vertex_buffer.set_data_f32(mesh.vertex_data());

        // Configure vertex attributes.
        let position = mesh.position();
        vertex_buffer.set_attrib_f32(
            "position",
            AttribLayout {
                elements: position.elements,
                stride: position.stride,
                offset: position.offset,
            });

        if let Some(normal) = mesh.normal() {
            vertex_buffer.set_attrib_f32(
                "normal",
                AttribLayout {
                    elements: normal.elements,
                    stride: normal.stride,
                    offset: normal.offset
                });
        }

        // TODO: Support multiple texcoords.
        if let Some(texcoord) = mesh.texcoord().first() {
            vertex_buffer.set_attrib_f32(
                "texcoord",
                AttribLayout {
                    elements: texcoord.elements,
                    stride: texcoord.stride,
                    offset: texcoord.offset,
                });
        }

        let mut index_buffer = IndexBuffer::new();
        index_buffer.set_data_u32(mesh.indices());

        let mesh_id = self.mesh_counter.next();

        self.meshes.insert(
            mesh_id,
            MeshData {
                vertex_buffer:      vertex_buffer,
                index_buffer:       index_buffer,
                position_attribute: mesh.position(),
                normal_attribute:   mesh.normal(),
                uv_attribute:       None,
                element_count:      mesh.indices().len(),
            });

        mesh_id
    }

    fn register_texture(&mut self, texture: &Texture2d) -> GpuTexture {
        let (format, internal_format) = match texture.format() {
            DataFormat::Rgb => (TextureFormat::Rgb, TextureInternalFormat::Rgb),
            DataFormat::Rgba => (TextureFormat::Rgba, TextureInternalFormat::Rgba),
            DataFormat::Bgr => (TextureFormat::Bgr, TextureInternalFormat::Rgb),
            DataFormat::Bgra => (TextureFormat::Bgra, TextureInternalFormat::Rgba),
        };

        // Create the Texture2d from the texture data.
        let texture_result = match texture.data() {
            &TextureData::f32(ref data) => {
                GlTexture2d::new(
                    format,
                    internal_format,
                    texture.width(),
                    texture.height(),
                    &*data)
            },
            &TextureData::u8(ref data) => {
                GlTexture2d::new(
                    format,
                    internal_format,
                    texture.width(),
                    texture.height(),
                    &*data)
            },
            &TextureData::u8x3(ref data) => {
                GlTexture2d::new(
                    format,
                    internal_format,
                    texture.width(),
                    texture.height(),
                    &*data)
            },
            &TextureData::u8x4(ref data) => {
                GlTexture2d::new(
                    format,
                    internal_format,
                    texture.width(),
                    texture.height(),
                    &*data)
            },
        };
        let gl_texture = texture_result.expect("Unable to send texture to GPU");

        // Register the mesh internally.
        let texture_id = self.texture_counter.next();

        let old = self.textures.insert(texture_id, gl_texture);
        assert!(old.is_none());

        texture_id
    }

    fn register_mesh_instance(&mut self, mesh_instance: MeshInstance) -> MeshInstanceId {
        let mesh_instance_id = self.mesh_instance_counter.next();

        let old = self.mesh_instances.insert(mesh_instance_id, mesh_instance);
        assert!(old.is_none());

        mesh_instance_id
    }

    fn get_mesh_instance(&self, id: MeshInstanceId) -> Option<&MeshInstance> {
        self.mesh_instances.get(&id)
    }

    fn get_mesh_instance_mut(&mut self, id: MeshInstanceId) -> Option<&mut MeshInstance> {
        self.mesh_instances.get_mut(&id)
    }

    fn register_anchor(&mut self, anchor: Anchor) -> AnchorId {
        let anchor_id = self.anchor_counter.next();

        let old = self.anchors.insert(anchor_id, anchor);
        assert!(old.is_none());

        anchor_id
    }

    fn get_anchor(&self, anchor_id: AnchorId) -> Option<&Anchor> {
        self.anchors.get(&anchor_id)
    }

    fn get_anchor_mut(&mut self, anchor_id: AnchorId) -> Option<&mut Anchor> {
        self.anchors.get_mut(&anchor_id)
    }

    fn register_camera(&mut self, camera: Camera) -> CameraId {
        let camera_id = self.camera_counter.next();

        let old = self.cameras.insert(camera_id, camera);
        assert!(old.is_none());

        camera_id
    }

    fn get_camera(&self, camera_id: CameraId) -> Option<&Camera> {
        self.cameras.get(&camera_id)
    }

    fn get_camera_mut(&mut self, camera_id: CameraId) -> Option<&mut Camera> {
        self.cameras.get_mut(&camera_id)
    }

    fn register_light(&mut self, light: Light) -> LightId {
        let light_id = self.light_counter.next();

        let old = self.lights.insert(light_id, light);
        assert!(old.is_none());

        light_id
    }

    fn get_light(&self, light_id: LightId) -> Option<&Light> {
        self.lights.get(&light_id)
    }

    fn get_light_mut(&mut self, light_id: LightId) -> Option<&mut Light> {
        self.lights.get_mut(&light_id)
    }
}

#[derive(Debug)]
pub enum Error {
    ContextError(ContextError),
}

impl From<ContextError> for Error {
    fn from(from: ContextError) -> Error {
        Error::ContextError(from)
    }
}

#[derive(Debug)]
struct MeshData {
    vertex_buffer: VertexBuffer,
    index_buffer: IndexBuffer,
    pub position_attribute: VertexAttribute,
    pub normal_attribute: Option<VertexAttribute>,
    pub uv_attribute: Option<VertexAttribute>,
    element_count: usize,
}
