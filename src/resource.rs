use std::collections::HashMap;
use std::io::prelude::*;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::error::Error;
use std::rc::Rc;
use std::cell::RefCell;

use collada::{self, COLLADA, GeometricElement, ArrayElement, PrimitiveType, VisualScene, Geometry,
              Node};

use polygon::gl_render::{GLRender, GLMeshData, ShaderProgram};
use polygon::geometry::mesh::Mesh;

use wav::Wave;
use scene::Scene;
use ecs::Entity;
use component::{MeshManager, TransformManager};

#[derive(Debug, Clone)]
pub struct ResourceManager {
    renderer: Rc<GLRender>,
    meshes: RefCell<HashMap<String, GLMeshData>>,
    audio_clips: RefCell<HashMap<String, Rc<Wave>>>,

    visual_scenes: RefCell<HashMap<String, VisualScene>>,
    geometries: RefCell<HashMap<String, Geometry>>,

    resource_path: RefCell<PathBuf>,
}

impl ResourceManager {
    pub fn new(renderer: Rc<GLRender>) -> ResourceManager {
        ResourceManager {
            renderer: renderer,
            meshes: RefCell::new(HashMap::new()),
            audio_clips: RefCell::new(HashMap::new()),

            visual_scenes: RefCell::new(HashMap::new()),
            geometries: RefCell::new(HashMap::new()),

            resource_path: RefCell::new(PathBuf::new()),
        }
    }

    /// TODO: Perform validity checking on data when loading (e.g. make sure all nodes have an id).
    pub fn load_model<P: AsRef<Path>>(&self, path: P) -> Result<(), String> {
        let mut visual_scenes = self.visual_scenes.borrow_mut();
        let mut geometries = self.geometries.borrow_mut();

        let mut full_path = self.resource_path.borrow().clone();
        full_path.push(path);
        let metadata = match fs::metadata(&full_path) {
            Err(why) => return Err(format!(
                "Unable to read metadata for {}, either it doesn't exist or the user lacks permissions, {}",
                full_path.display(),
                &why)),
            Ok(metadata) => metadata,
        };
        if !metadata.is_file() {
            return Err(format!(
                "{} could not be loaded because it is not a file",
                full_path.display()));
        }
        let collada_data = match COLLADA::load(&full_path) {
            Err(why) => return Err(format!(
                "couldn't open {}: {}",
                full_path.display(),
                &why)),
            Ok(data) => data,
        };

        // Store each of the visual scenes from the collada file.
        for visual_scene in collada_data.library_visual_scenes.as_ref().unwrap().visual_scenes.iter() {
            let id = match visual_scene.id {
                None => return Err(format!(
                    "COLLADA file {} contained a <visual_scene> with no \"id\" attribute, this is unsupported.",
                    full_path.display())),
                Some(ref id) => id.clone(),
            };
            visual_scenes.insert(id, visual_scene.clone());
        }

        // Store each of the geometries so they can be referenced later.
        for geometry in collada_data.library_geometries.as_ref().unwrap().geometries.iter() {
            let id = match geometry.id {
                None => return Err(format!(
                    "COLLADA file {} contained a <geometry> with no \"id\" attribute, this is unsupported",
                    full_path.display())),
                Some(ref id) => id.clone(),
            };
            geometries.insert(id, geometry.clone());
        }

        Ok(())
    }

    /// Sets the path to the resources director.
    ///
    /// # Details
    ///
    /// The resource manager is configured to look in the specified directory when loading
    /// resources such as meshes and shaders.
    pub fn set_resource_path<P: AsRef<Path>>(&self, path: P) {
        let mut resource_path = self.resource_path.borrow_mut();
        *resource_path = PathBuf::new();
        resource_path.push(path);
    }

    pub fn get_mesh(&self, uri: &str) -> Result<GLMeshData, String> {
        // Use cached mesh data if possible.
        if let Some(mesh) = self.get_cached_mesh(uri) {
            return Ok(mesh);
        }

        // Generate mesh data since none has ben created previously.
        let visual_scenes = self.visual_scenes.borrow();

        // TODO: Handle invalid URIs (empty, invalid characters?).
        let mut uri_segments = uri.split(".");
        let root = uri_segments.next().unwrap();
        let visual_scene = match visual_scenes.get(root) {
            None => return Err(format!(
                "No source file {} found from which to load {}",
                root,
                uri)),
            Some(visual_scene) => visual_scene,
        };

        // Get the first node in the URI.
        let mut node = {
            let name = uri_segments.next().unwrap();
            let mut result: Option<&Node> = None;
            for node in &visual_scene.nodes {
                if node.id.as_ref().unwrap() == name {
                    result = Some(node);
                    break;
                }
            }

            match result {
                None => return Err(format!(
                    "No node named {} found in scene {}",
                    name,
                    root)),
                Some(node) => node,
            }
        };

        for name in uri_segments {
            let mut result: Option<&Node> = None;
            for next_node in &node.nodes {
                if next_node.id.as_ref().unwrap() == name {
                    result = Some(next_node);
                    break;
                }
            }

            match result {
                None => return Err(format!(
                    "No node named {} found when parsing {}",
                    name,
                    uri)),
                Some(next_node) =>
                    node = next_node,
            }
        }

        let mesh_data = self.gen_mesh_from_node(node, uri).unwrap();
        Ok(mesh_data)
    }

    pub fn get_audio_clip(&self, path_text: &str) -> Rc<Wave> {
        let mut audio_clips = self.audio_clips.borrow_mut();

        if !audio_clips.contains_key(path_text) {
            let wave = Wave::from_file(path_text).unwrap();
            audio_clips.insert(path_text.into(), Rc::new(wave));
        }

        audio_clips.get(path_text).unwrap().clone()
    }

    pub fn instantiate_model(&self, resource: &str, scene: &Scene) -> Result<Entity, String> {
        if resource.contains(".") {
            println!("WARNING: ResourceManager::instantiate_model() doesn't yet support fully qualified URIs, only root assets may be instantiated.");
        }

        let mut uri_segments = resource.split(".");
        let root = uri_segments.next().unwrap();
        let visual_scenes = self.visual_scenes.borrow();
        let visual_scene = {
            match visual_scenes.get(root) {
                None => return Err(format!(
                    "No source file {} found from which to load {}",
                    root,
                    resource)),
                Some(visual_scene) => visual_scene,
            }
        };

        let node = {
            if visual_scene.nodes.len() == 0 {
                return Err(format!(
                    "No nodes associated with model {}",
                    resource));
            }

            if visual_scene.nodes.len() > 1 {
                println!(
                    "WARNING: Model {} has more than one node at the root level. This is not currenlty supported, only the first node will be used.",
                    resource);
            }

            &visual_scene.nodes[0]
        };

        let mut uri = String::from(resource);
        uri.push_str(".");
        uri.push_str(node.id.as_ref().unwrap());

        let mesh_data = if let Some(mesh_data) = self.get_cached_mesh(&uri) {
            mesh_data
        } else {
            match self.gen_mesh_from_node(node, &uri) {
                Err(message) => return Err(message),
                Ok(mesh_data) => mesh_data,
            }
        };

        let entity = scene.create_entity();
        let mut transform_manager = scene.get_manager_mut::<TransformManager>();
        transform_manager.assign(entity);
        scene.get_manager_mut::<MeshManager>().give_mesh(entity, mesh_data);

        return Ok(entity);
    }

    pub fn get_shader<P: AsRef<Path>>(
        &self,
        shader_path: P
    ) -> Result<ShaderProgram, ParseShaderError> {
        let mut full_path = self.resource_path.borrow().clone();
        full_path.push(shader_path);
        let program_src = load_file_text(full_path);

        let programs = try!(ShaderParser::parse(&*program_src));
        let vert_src = match programs.iter().find(|program| program.name == "vert") {
            None => return Err(ParseShaderError::NoVertProgram),
            Some(program) => program.src,
        };

        let frag_src = match programs.iter().find(|program| program.name == "frag") {
            None => return Err(ParseShaderError::NoFragProgram),
            Some(program) => program.src,
        };

        Ok(self.renderer.compile_shader_program(vert_src, frag_src))
    }

    fn gen_mesh_from_node(&self, node: &collada::Node, uri: &str) -> Result<GLMeshData, String> {
        let geometry_name = {
            if node.instance_geometries.len() == 0 {
                return Err(format!("No geometry is identified by {}", uri));
            }
            if node.instance_geometries.len() > 1 {
                return Err(format!("More than one geometry is identified by {}", uri));
            }

            let url = &node.instance_geometries[0].url;
            &url[1..] // Skip the leading "#" character that starts all URLs.
        };

        let geometries = self.geometries.borrow();
        let geometry = geometries.get(geometry_name).unwrap();
        self.gen_mesh(geometry, uri)
    }

    fn has_cached_mesh(&self, uri: &str) -> bool {
        self.meshes.borrow().contains_key(uri)
    }

    fn get_cached_mesh(&self, uri: &str) -> Option<GLMeshData> {
        match self.meshes.borrow().get(uri) {
            None => None,
            Some(mesh_ref) => Some(*mesh_ref),
        }
    }

    fn gen_mesh(&self, geometry: &Geometry, uri: &str) -> Result<GLMeshData, String> {
        assert!(!self.has_cached_mesh(uri), "Attempting to create a new mesh for {} when the uri is already in the meshes map", uri);

        let mesh = geometry_to_mesh(geometry);

        let mesh_data = self.renderer.gen_mesh(&mesh);
        self.meshes.borrow_mut().insert(uri.into(), mesh_data);

        Ok(mesh_data)
    }
}

/// Load the mesh data from a COLLADA .dae file.
///
/// The data in a COLLADA files is formatted for efficiency, but isn't necessarily
/// organized in a way that is supported by the graphics API. This method reformats the
/// data so that it can be sent straight to the GPU without further manipulation.
///
/// In order to to this, it reorganizes the normals, UVs, and other vertex attributes to
/// be in the same order as the vertex positions.
fn geometry_to_mesh(geometry: &Geometry) -> Mesh {
    let mesh = match geometry.data {
        GeometricElement::Mesh(ref mesh) => mesh,
        _ => panic!("No mesh found within geometry")
    };

    let position_data_raw = get_raw_positions(&mesh);
    let normal_data_raw = get_normals(&mesh);

    let triangles = match mesh.primitives[0] {
        PrimitiveType::Triangles(ref triangles) => triangles,
        _ => panic!("Only triangles primitives are supported currently")
    };
    let primitive_indices = &triangles.primitives;

    // Create a new array for the positions so we can add the w coordinate.
    let mut position_data: Vec<f32> = Vec::with_capacity(position_data_raw.len() / 3 * 4);

    // Create a new array for the normals and rearrange them to match the order of position attributes.
    let mut normal_data: Vec<f32> = Vec::with_capacity(position_data.len());

    // Iterate over the indices, rearranging the normal data to match the position data.
    let stride = triangles.inputs.len();
    let mut vertex_index_map: HashMap<(usize, usize), u32> = HashMap::new();
    let mut indices: Vec<u32> = Vec::new();
    let vertex_count = triangles.count * 3;
    let mut index_count = 0;
    for offset in 0..vertex_count {
        // Determine the offset of the the current vertex's attributes
        let position_index = primitive_indices[offset * stride];
        let normal_index = primitive_indices[offset * stride + 1];

        // Push the index of the vertex, either reusing an existing vertex or creating a new one.
        let vertex_key = (position_index, normal_index);
        let vertex_index = if vertex_index_map.contains_key(&vertex_key) {
            // Vertex has already been assembled, reuse the index.
            (*vertex_index_map.get(&vertex_key).unwrap()) as u32
        } else {
            // Assemble new vertex.
            let vertex_index = index_count;
            index_count += 1;
            vertex_index_map.insert(vertex_key, vertex_index as u32);

            // Append position to position data array.
            for offset in 0..3 {
                position_data.push(position_data_raw[position_index * 3 + offset]);
            }
            position_data.push(1.0);

            // Append normal to normal data array.
            for offset in 0..3 {
                normal_data.push(normal_data_raw[normal_index * 3 + offset]);
            }

            vertex_index
        };

        indices.push(vertex_index);
    }

    let mesh = Mesh::from_raw_data(position_data.as_ref(), normal_data.as_ref(), indices.as_ref());

    mesh
}

fn get_raw_positions(mesh: &collada::Mesh) -> &[f32] {
    // TODO: Consult the correct element (<triangles> for now) to determine which source has position data.
    let position_data: &[f32] = match mesh.sources[0].array_element {
        ArrayElement::Float(ref float_array) => float_array.as_ref(),
        _ => panic!("Only float arrays supported for vertex position array")
    };
    assert!(position_data.len() > 0);

    position_data
}

fn get_normals(mesh: &collada::Mesh) -> &[f32] {
    // TODO: Consult the correct element (<triangles> for now) to determine which source has normal data.
    let normal_data: &[f32] = match mesh.sources[1].array_element {
        ArrayElement::Float(ref float_array) => float_array.as_ref(),
        _ => panic!("Only float arrays supported for vertex normal array")
    };
    assert!(normal_data.len() > 0);

    normal_data
}

pub fn load_file_text<P: AsRef<Path>>(file_path: P) -> String {
    let mut file = match File::open(&file_path) {
        // The `desc` field of `IoError` is a string that describes the error
        Err(why) => panic!("couldn't open {}: {}", file_path.as_ref().display(), Error::description(&why)),
        Ok(file) => file,
    };
    let mut contents = String::new();
    match file.read_to_string(&mut contents) {
        Err(why) => panic!("couldn't read {}: {}", file_path.as_ref().display(), Error::description(&why)),
        Ok(_) => ()
    }
    contents
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ParseShaderError {
    NoVertProgram,
    NoFragProgram,
    MultipleVertShader,
    MultipleFragShader,
    ProgramMissingName,
    UnmatchedBraces,
    MissingOpeningBrace,
    CompileError(String),
    LinkError(String),
}

#[derive(Debug, Clone)]
struct ShaderParser;

#[derive(Debug, Clone)]
struct ShaderProgramSrc<'a> {
    name: &'a str,
    src: &'a str,
}

impl ShaderParser {
    fn parse(shader_src: &str) -> Result<Vec<ShaderProgramSrc>, ParseShaderError> {
        let mut programs: Vec<ShaderProgramSrc> = Vec::new();
        let mut index = 0;
        loop {
            let substr = &shader_src[index..];
            let (program, end_index) = try!(ShaderParser::parse_program(substr));
            programs.push(program);
            index = end_index;

            if programs.len() >= 2 {
                break;
            }
        }

        Ok(programs)
    }

    fn parse_program(src: &str) -> Result<(ShaderProgramSrc, usize), ParseShaderError> {
        if let Some(index) = src.find("program") {
            let program_src = src[index..].trim_left();
            let program_name = match program_src.split_whitespace().nth(1) {
                Some(name) => name,
                None => return Err(ParseShaderError::ProgramMissingName),
            };

            let (program_src, end_index) = match program_src.find('{') {
                None => return Err(ParseShaderError::MissingOpeningBrace),
                Some(index) => {
                    let (src, index) = try!(ShaderParser::parse_braces_contents(&program_src[index..]));
                    (src.trim(), index)
                }
            };

            let program = ShaderProgramSrc {
                name: program_name,
                src: program_src,
            };
            Ok((program, end_index))
        } else {
            return Err(ParseShaderError::NoVertProgram);
        }
    }

    /// Parses the contents of a curly brace-delimeted block.
    ///
    /// Retuns a substring of the source string that contains the contents of the block without
    /// the surrounding curly braces. Fails if there is no matching close brace.
    fn parse_braces_contents(src: &str) -> Result<(&str, usize), ParseShaderError> {
        assert!(src.starts_with("{"));

        let mut depth = 0;
        for (index, character) in src.chars().enumerate() {
            match character {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        // We're at the end.
                        return Ok((&src[1..index], index));
                    }
                },
                _ => {}
            }
        }

        // Uh-oh, we got to the end and never closed the braces.
        Err(ParseShaderError::UnmatchedBraces)
    }
}
