//! Contains the plugin and its helper types.
//!
//! The [`ShapePlugin`] provides the creation of shapes with minimal
//! boilerplate.
//!
//! ## How it works
//! The user spawns a [`ShapeBundle`](crate::entity::ShapeBundle) from a
//! system in the `UPDATE` stage.
//!
//! Then, in [`Stage::Shape`] stage, there is a system
//! that creates a mesh for each entity that has been spawned as a
//! `ShapeBundle`.

use bevy::{
    app::{App, Plugin},
    asset::Assets,
    ecs::{
        query::{Changed, Or},
        schedule::{StageLabel, SystemStage},
        system::{Query, ResMut},
    },
    log::error,
    render::{
        mesh::{Indices, Mesh},
        render_resource::PrimitiveTopology,
    },
    sprite::Mesh2dHandle,
};
use lyon_tessellation::{self as tess, BuffersBuilder, FillTessellator, StrokeTessellator};

use crate::{
    draw::{DrawMode, FillMode, StrokeMode},
    entity::Path,
    render::RenderShapePlugin,
    vertex::{VertexBuffers, VertexConstructor},
};

/// Stages for this plugin.
#[derive(Debug, Clone, Eq, Hash, PartialEq, StageLabel)]
pub enum Stage {
    /// The stage where the [`ShapeBundle`](crate::entity::ShapeBundle) gets
    /// completed.
    Shape,
}

/// A plugin that provides resources and a system to draw shapes in Bevy with
/// less boilerplate.
pub struct ShapePlugin;

impl Plugin for ShapePlugin {
    fn build(&self, app: &mut App) {
        let fill_tess = FillTessellator::new();
        let stroke_tess = StrokeTessellator::new();
        app.insert_resource(fill_tess)
            .insert_resource(stroke_tess)
            .add_stage_after(
                bevy::app::CoreStage::Update,
                Stage::Shape,
                SystemStage::parallel(),
            )
            .add_system_to_stage(Stage::Shape, mesh_shapes_system)
            .add_plugin(RenderShapePlugin);
    }
}

/// Queries all the [`ShapeBundle`]s to mesh them when they are added
/// or re-mesh them when they are changed.
#[allow(clippy::type_complexity)]
fn mesh_shapes_system(
    mut meshes: ResMut<Assets<Mesh>>,
    mut fill_tess: ResMut<FillTessellator>,
    mut stroke_tess: ResMut<StrokeTessellator>,
    mut query: Query<(&DrawMode, &Path, &mut Mesh2dHandle), Or<(Changed<Path>, Changed<DrawMode>)>>,
) {
    for (tess_mode, path, mut mesh) in query.iter_mut() {
        let mut buffers = VertexBuffers::new();

        match tess_mode {
            DrawMode::Fill(mode) => {
                fill(&mut fill_tess, &path.0, mode, &mut buffers);
            }
            DrawMode::Stroke(mode) => {
                stroke(&mut stroke_tess, &path.0, mode, &mut buffers);
            }
            DrawMode::Outlined {
                fill_mode,
                outline_mode,
            } => {
                fill(&mut fill_tess, &path.0, fill_mode, &mut buffers);
                stroke(&mut stroke_tess, &path.0, outline_mode, &mut buffers);
            }
        }

        mesh.0 = meshes.add(build_mesh(&buffers));
    }
}

/// A wrapper around [`BuffersBuilder`] to generate counter-clockwise (Ccw)
/// meshes. The lyon crate always generates clockwise (Cw) meshes (for now,
/// might change in version 0.18) so swapping two vertices always allows us to
/// build the Ccw mesh. More info: https://github.com/nical/lyon/issues/717#issuecomment-934360057
struct CcwBuffersBuilder<'l, OutputVertex, OutputIndex, Ctor>(
    BuffersBuilder<'l, OutputVertex, OutputIndex, Ctor>,
);

impl<'l, OutputVertex, OutputIndex, Ctor> tess::GeometryBuilder
    for CcwBuffersBuilder<'l, OutputVertex, OutputIndex, Ctor>
where
    OutputVertex: 'l,
    OutputIndex: std::ops::Add + From<tess::VertexId> + tess::geometry_builder::MaxIndex,
{
    fn begin_geometry(&mut self) {
        self.0.begin_geometry();
    }

    fn end_geometry(&mut self) -> tess::Count {
        self.0.end_geometry()
    }

    fn abort_geometry(&mut self) {
        self.0.abort_geometry();
    }

    fn add_triangle(&mut self, a: tess::VertexId, b: tess::VertexId, c: tess::VertexId) {
        self.0.add_triangle(a, c, b);
    }
}

impl<'l, OutputVertex, OutputIndex, Ctor> tess::FillGeometryBuilder
    for CcwBuffersBuilder<'l, OutputVertex, OutputIndex, Ctor>
where
    OutputVertex: 'l,
    OutputIndex: std::ops::Add + From<tess::VertexId> + tess::geometry_builder::MaxIndex,
    Ctor: tess::FillVertexConstructor<OutputVertex>,
{
    fn add_fill_vertex(
        &mut self,
        vertex: tess::FillVertex,
    ) -> Result<tess::VertexId, tess::geometry_builder::GeometryBuilderError> {
        self.0.add_fill_vertex(vertex)
    }
}

impl<'l, OutputVertex, OutputIndex, Ctor> tess::StrokeGeometryBuilder
    for CcwBuffersBuilder<'l, OutputVertex, OutputIndex, Ctor>
where
    OutputVertex: 'l,
    OutputIndex: std::ops::Add + From<tess::VertexId> + tess::geometry_builder::MaxIndex,
    Ctor: tess::StrokeVertexConstructor<OutputVertex>,
{
    fn add_stroke_vertex(
        &mut self,
        vertex: tess::StrokeVertex,
    ) -> Result<tess::VertexId, tess::geometry_builder::GeometryBuilderError> {
        self.0.add_stroke_vertex(vertex)
    }
}

#[allow(clippy::trivially_copy_pass_by_ref)] // lyon takes &FillOptions
fn fill(
    tess: &mut ResMut<FillTessellator>,
    path: &tess::path::Path,
    mode: &FillMode,
    buffers: &mut VertexBuffers,
) {
    if let Err(e) = tess.tessellate_path(
        path,
        &mode.options,
        &mut CcwBuffersBuilder(BuffersBuilder::new(
            buffers,
            VertexConstructor { color: mode.color },
        )),
    ) {
        error!("FillTessellator error: {:?}", e);
    }
}

#[allow(clippy::trivially_copy_pass_by_ref)] // lyon takes &StrokeOptions
fn stroke(
    tess: &mut ResMut<StrokeTessellator>,
    path: &tess::path::Path,
    mode: &StrokeMode,
    buffers: &mut VertexBuffers,
) {
    if let Err(e) = tess.tessellate_path(
        path,
        &mode.options,
        &mut CcwBuffersBuilder(BuffersBuilder::new(
            buffers,
            VertexConstructor { color: mode.color },
        )),
    ) {
        error!("StrokeTessellator error: {:?}", e);
    }
}

fn build_mesh(buffers: &VertexBuffers) -> Mesh {
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList);
    mesh.set_indices(Some(Indices::U32(buffers.indices.clone())));
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_POSITION,
        buffers
            .vertices
            .iter()
            .map(|v| [v.position[0], v.position[1], 0.0])
            .collect::<Vec<[f32; 3]>>(),
    );
    mesh.insert_attribute(
        Mesh::ATTRIBUTE_COLOR,
        buffers
            .vertices
            .iter()
            .map(|v| v.color)
            .collect::<Vec<u32>>(),
    );

    mesh
}
