use halcyon_data::NavMesh;
use std::{env, error::Error, fs};

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args().skip(1);
    let path = args
        .next()
        .ok_or("usage: navmesh_query FILE X Z [X Z ...]")?;
    let coordinates = args
        .map(|arg| arg.parse::<f32>())
        .collect::<Result<Vec<_>, _>>()?;
    let (points, remaining) = coordinates.as_chunks::<2>();
    if points.is_empty() || !remaining.is_empty() || coordinates.iter().any(|p| !p.is_finite()) {
        return Err("supply one or more finite X Z coordinate pairs".into());
    }
    let mesh = NavMesh::parse(&fs::read(path)?)?;
    println!("tiles={}", mesh.tile_count());
    println!("x,z,ground_y");
    for &[x, z] in points {
        if let Some(y) = mesh.height_at(x, z) {
            println!("{x},{z},{y}");
        } else {
            println!("{x},{z},off_mesh");
        }
    }
    Ok(())
}
