//! Boolean combination of planar-faced solids.
//!
//! # The algorithm in one paragraph
//!
//! Collect every distinct plane that carries a face of either solid. For each such plane, cut
//! *both* solids with it — that gives the material each one occupies in that plane — and combine
//! those two 2D regions with the planar boolean. Whatever is left becomes one face.
//!
//! # Why that shape of algorithm
//!
//! - **Coplanar faces fall out for free.** Two boxes sharing a face plane produce one face, not
//!   two that then have to be merged and re-welded, and no sliver survives along the seam.
//! - **It is symmetric.** For union and intersection the plane list and the two sections are the
//!   same whichever way round the operands go, so `a ∪ b` and `b ∪ a` agree.
//! - **It fails loudly.** An empty or degenerate combination produces no face for that plane; the
//!   result is validated as a whole afterwards, so a broken shell is an error, not a body.
//!
//! The result's outward direction is settled by measuring its volume and flipping if it came out
//! inside-out, rather than by tracking which operand each plane came from — that is where the
//! sign of a cut is easy to get backwards.

use cadcraft_geom::{Regions, bool2d};

use crate::{Face, Plane, Solid, SolidError, limits::MAX_FACES, section::section};

/// How two solids combine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bool3 {
    Union,
    Intersect,
    Difference,
}

/// One plane of the result, with what the operands say about it.
struct Slot {
    plane: Plane,
    /// A has a face in this plane.
    a_owns: bool,
    /// B has a face in this plane.
    b_owns: bool,
    /// Where both do: do their normals point the same way (same side of the plane)?
    same_side: bool,
}

/// Boolean combination of two solids.
///
/// `a - b` is [`Bool3::Difference`]. Operands must overlap for union and intersection to produce
/// anything; disjoint operands give an empty result for intersection and a union of the two.
pub fn boolean(a: &Solid, b: &Solid, op: Bool3) -> Result<Solid, SolidError> {
    if !a.bounds().intersects(&b.bounds()) {
        return match op {
            Bool3::Union => {
                let mut faces = a.faces.clone();
                faces.extend(b.faces.iter().cloned());
                Solid::from_faces(faces)
            }
            Bool3::Intersect => Err(SolidError::Empty),
            Bool3::Difference => Ok(a.clone()),
        };
    }
    let tol = a.tolerance().max(b.tolerance());
    let dir_tol = 1e-9;

    // Every distinct plane carrying a face of either operand, remembering which operand owns it
    // and whether their normals agree.
    //
    // Ownership decides *whether* there is a face there at all (a plane that merely cuts through
    // the middle of a solid is not a boundary of the result). Agreement decides *what* it is when
    // both operands own the plane: two solids sharing a face plane and sitting on the same side
    // contribute one merged face, while two touching along that plane each keep their own side.
    // Subtracting a slab that merely touches must leave the original untouched, and getting that
    // wrong deletes a whole face.
    let mut planes: Vec<Slot> = Vec::new();
    for (solid, is_a) in [(a, true), (b, false)] {
        for f in &solid.faces {
            match planes.iter_mut().find(|s| s.plane.coplanar(&f.plane, tol, dir_tol)) {
                Some(slot) => {
                    let same = slot.plane.normal.dot(f.plane.normal) > 0.0;
                    if is_a {
                        slot.a_owns = true;
                    } else {
                        slot.b_owns = true;
                    }
                    slot.same_side = same;
                }
                None => planes.push(Slot { plane: f.plane, a_owns: is_a, b_owns: !is_a, same_side: true }),
            }
        }
        if planes.len() > MAX_FACES {
            return Err(SolidError::TooManyFaces(planes.len(), MAX_FACES));
        }
    }

    let mut faces: Vec<Face> = Vec::new();
    for slot in &planes {
        let (p, a_owns, b_owns, same_side) = (&slot.plane, slot.a_owns, slot.b_owns, slot.same_side);
        let sa = section(a, p, tol)?.region;
        let sb = section(b, p, tol)?.region;
        let r: Option<Regions> = match (op, a_owns, b_owns, same_side) {
            // One side owns the plane: that operand's face survives only where the other is not.
            (Bool3::Union, true, false, _) => bool2d::difference(&sa, &sb),
            (Bool3::Union, false, true, _) => bool2d::difference(&sb, &sa),
            // Both own it, same side: one merged face. Opposite sides: the plane runs through the
            // middle of the result and is not a boundary at all.
            (Bool3::Union, true, true, true) => bool2d::union(&sa, &sb),
            (Bool3::Union, true, true, false) => Some(Regions::default()),
            (Bool3::Intersect, true, true, true) => bool2d::intersect(&sa, &sb),
            (Bool3::Intersect, true, false, _) | (Bool3::Intersect, false, true, _) => bool2d::intersect(&sa, &sb),
            (Bool3::Intersect, true, true, false) | (Bool3::Intersect, false, false, _) => Some(Regions::default()),
            // A - B: A's own face survives where B does not cover it; B's face survives only where
            // A is there to cut against.
            (Bool3::Difference, true, false, _) => bool2d::difference(&sa, &sb),
            (Bool3::Difference, false, true, true) => bool2d::intersect(&sb, &sa),
            // Same side: where both have material the result has none (B removed it), so only
            // A's uncovered part survives. Opposite sides: B is not covering A's face at all.
            (Bool3::Difference, true, true, true) => bool2d::difference(&sa, &sb),
            // Opposite sides: B is not covering any of A's face, so A's face is untouched.
            (Bool3::Difference, true, true, false) => Some(sa.clone()),
            (Bool3::Difference, false, true, false) | (Bool3::Difference, false, false, _) | (Bool3::Union, false, false, _) => {
                Some(Regions::default())
            }
        };
        let Some(region) = r else {
            return Err(SolidError::Invalid("the planar boolean on a section failed".into()));
        };
        if region.is_empty() {
            continue;
        }
        faces.push(Face::new(*p, region)?);
    }
    let mut solid = Solid::from_faces(faces)?;
    solid.orient_faces()?;
    solid.validate()?;
    Ok(solid)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Plane, extrude, tests_support::box_at, tests_support::must, tests_support::rect, tests_support::rect_with_hole};
    use cadcraft_geom::Vec3;

    #[test]
    fn two_disjoint_boxes_union_into_one_solid() {
        let a = box_at(Vec3::ZERO, Vec3::new(2.0, 2.0, 2.0));
        let b = box_at(Vec3::new(5.0, 0.0, 0.0), Vec3::new(7.0, 2.0, 2.0));
        let u = must(boolean(&a, &b, Bool3::Union), "union");
        u.validate().unwrap_or_else(|e| panic!("{e}"));
        assert!((u.volume() - 16.0).abs() < 1e-9, "volume {}", u.volume());
        let (faces, _, _) = u.counts();
        assert_eq!(faces, 12, "two separate boxes");
    }

    #[test]
    fn two_disjoint_boxes_intersect_to_nothing() {
        let a = box_at(Vec3::ZERO, Vec3::new(2.0, 2.0, 2.0));
        let b = box_at(Vec3::new(5.0, 0.0, 0.0), Vec3::new(7.0, 2.0, 2.0));
        assert!(boolean(&a, &b, Bool3::Intersect).is_err(), "an empty result is an error, not an empty solid");
    }

    #[test]
    fn a_cube_cut_by_half_its_volume() {
        let a = box_at(Vec3::ZERO, Vec3::new(4.0, 4.0, 4.0));
        let b = box_at(Vec3::new(2.0, -1.0, -1.0), Vec3::new(6.0, 5.0, 5.0));
        let d = must(boolean(&a, &b, Bool3::Difference), "difference");
        d.validate().unwrap_or_else(|e| panic!("{e}"));
        assert!((d.volume() - 32.0).abs() < 1e-9, "volume {}", d.volume());
        assert!(d.contains(Vec3::new(1.0, 2.0, 2.0)), "the untouched half survives");
        assert!(!d.contains(Vec3::new(3.0, 2.0, 2.0)), "the cut half is gone");
        d.validate_vertices().unwrap_or_else(|e| panic!("{e}"));
    }

    #[test]
    fn two_overlapping_boxes_intersect_to_a_box() {
        let a = box_at(Vec3::ZERO, Vec3::new(4.0, 4.0, 4.0));
        let b = box_at(Vec3::new(2.0, 2.0, 2.0), Vec3::new(6.0, 6.0, 6.0));
        let i = must(boolean(&a, &b, Bool3::Intersect), "intersect");
        i.validate().unwrap_or_else(|e| panic!("{e}"));
        assert!((i.volume() - 8.0).abs() < 1e-9, "volume {}", i.volume());
        assert!(i.contains(Vec3::new(3.0, 3.0, 3.0)));
        assert!(!i.contains(Vec3::new(1.0, 1.0, 1.0)));
    }

    #[test]
    fn two_overlapping_boxes_union_is_convex_again() {
        let a = box_at(Vec3::ZERO, Vec3::new(4.0, 4.0, 4.0));
        let b = box_at(Vec3::new(2.0, 2.0, 2.0), Vec3::new(6.0, 6.0, 6.0));
        let u = must(boolean(&a, &b, Bool3::Union), "union");
        u.validate().unwrap_or_else(|e| panic!("{e}"));
        assert!((u.volume() - (64.0 + 64.0 - 8.0)).abs() < 1e-9, "volume {}", u.volume());
        u.validate_vertices().unwrap_or_else(|e| panic!("{e}"));
    }

    #[test]
    fn boolean_is_commutative_for_union_and_intersection() {
        let a = box_at(Vec3::ZERO, Vec3::new(4.0, 3.0, 2.0));
        let b = box_at(Vec3::new(2.0, 1.0, 1.0), Vec3::new(6.0, 5.0, 3.0));
        for op in [Bool3::Union, Bool3::Intersect] {
            let (Ok(x), Ok(y)) = (boolean(&a, &b, op), boolean(&b, &a, op)) else {
                panic!("{op:?} failed");
            };
            assert!((x.volume() - y.volume()).abs() < 1e-9, "{op:?}: {} vs {}", x.volume(), y.volume());
        }
    }

    #[test]
    fn subtracting_a_part_that_only_touches_gives_the_original() {
        let a = box_at(Vec3::ZERO, Vec3::new(4.0, 4.0, 4.0));
        let b = box_at(Vec3::new(4.0, 0.0, 0.0), Vec3::new(6.0, 4.0, 4.0));
        let d = must(boolean(&a, &b, Bool3::Difference), "difference");
        assert!((d.volume() - 64.0).abs() < 1e-9, "volume {}", d.volume());
    }

    #[test]
    fn a_pocket_cut_through_a_plate() {
        let plate = must(extrude(&rect(0.0, 0.0, 20.0, 20.0), &Plane::xy(0.0), 4.0), "extrude");
        let tool = must(extrude(&rect(5.0, 5.0, 15.0, 15.0), &Plane::xy(-1.0), 10.0), "extrude");
        let d = must(boolean(&plate, &tool, Bool3::Difference), "pocket cut");
        d.validate().unwrap_or_else(|e| panic!("{e}"));
        d.validate_vertices().unwrap_or_else(|e| panic!("{e}"));
        assert!((d.volume() - (400.0 * 4.0 - 100.0 * 4.0)).abs() < 1e-9, "volume {}", d.volume());
        assert!(!d.contains(Vec3::new(10.0, 10.0, 2.0)), "the pocket is open");
        assert!(d.contains(Vec3::new(2.0, 2.0, 2.0)));
        assert!(!d.contains(Vec3::new(10.0, 10.0, 20.0)), "the tool went past the top");
    }

    #[test]
    fn a_slot_cut_partway_through() {
        let plate = must(extrude(&rect(0.0, 0.0, 20.0, 20.0), &Plane::xy(0.0), 4.0), "extrude");
        let slot = must(extrude(&rect(5.0, 0.0, 7.0, 20.0), &Plane::xy(0.0), 2.0), "extrude");
        let d = must(boolean(&plate, &slot, Bool3::Difference), "slot cut");
        d.validate().unwrap_or_else(|e| panic!("{e}"));
        assert!((d.volume() - (400.0 * 4.0 - 2.0 * 20.0 * 2.0)).abs() < 1e-9, "volume {}", d.volume());
        assert!(!d.contains(Vec3::new(6.0, 10.0, 1.0)), "the slot is open below");
        assert!(d.contains(Vec3::new(6.0, 10.0, 3.0)), "material remains above the slot");
        assert!(d.contains(Vec3::new(1.0, 10.0, 1.0)), "material remains beside the slot");
    }

    #[test]
    fn a_fill_the_exact_size_of_a_hole_changes_nothing() {
        // Every wall of the tool lines up with a wall of the hole, so each cutting plane is tangent
        // to the plate along a whole face. This is where a naive section invents a hole that is not
        // there. Subtracting a tool that already sits in the void must leave the plate untouched.
        let plate = must(extrude(&rect_with_hole(0.0, 0.0, 20.0, 20.0, 6.0, 6.0, 14.0, 14.0), &Plane::xy(0.0), 3.0), "extrude");
        plate.validate().unwrap_or_else(|e| panic!("plate: {e}"));
        let fill = must(extrude(&rect(6.0, 6.0, 14.0, 14.0), &Plane::xy(0.0), 3.0), "extrude");
        let d = must(boolean(&plate, &fill, Bool3::Difference), "fill");
        d.validate().unwrap_or_else(|e| panic!("{e}"));
        d.validate_vertices().unwrap_or_else(|e| panic!("{e}"));
        assert!((d.volume() - (400.0 - 64.0) * 3.0).abs() < 1e-9, "volume {}", d.volume());
    }

    #[test]
    fn cutting_a_box_with_itself_removes_everything() {
        let a = box_at(Vec3::ZERO, Vec3::new(2.0, 2.0, 2.0));
        assert!(boolean(&a, &a, Bool3::Difference).is_err());
    }

    #[test]
    fn intersecting_a_box_with_itself_gives_the_box() {
        let a = box_at(Vec3::ZERO, Vec3::new(2.0, 3.0, 4.0));
        let i = must(boolean(&a, &a, Bool3::Intersect), "intersect");
        assert!((i.volume() - 24.0).abs() < 1e-9, "volume {}", i.volume());
        let u = must(boolean(&a, &a, Bool3::Union), "union");
        assert!((u.volume() - 24.0).abs() < 1e-9, "volume {}", u.volume());
    }

    #[test]
    fn booleans_at_a_large_scale_keep_their_volume() {
        let base = Vec3::new(1e5, -1e5, 1e5);
        let a = box_at(base, base + Vec3::new(10.0, 10.0, 10.0));
        let b = box_at(base + Vec3::new(5.0, 5.0, 5.0), base + Vec3::new(15.0, 15.0, 15.0));
        let d = must(boolean(&a, &b, Bool3::Difference), "difference");
        // A is 10x10x10 and the corner 5x5x5 of it lies inside B.
        assert!((d.volume() - 875.0).abs() < 1e-6, "volume {}", d.volume());
    }
}

#[cfg(test)]
mod debug_tests {
    use super::*;
    use crate::{
        Plane, extrude,
        section::section,
        tests_support::{must, rect},
    };

    #[test]
    fn debug_pocket2() {
        let plate = must(extrude(&rect(0.0, 0.0, 20.0, 20.0), &Plane::xy(0.0), 4.0), "plate");
        let tool = must(extrude(&rect(5.0, 5.0, 15.0, 15.0), &Plane::xy(-1.0), 10.0), "tool");
        let tol = plate.tolerance().max(tool.tolerance());
        println!("plate tol {:e} tool tol {:e}", plate.tolerance(), tool.tolerance());
        let mut planes: Vec<(Plane, bool, bool)> = Vec::new();
        for (s, is_a) in [(&plate, true), (&tool, false)] {
            for f in &s.faces {
                match planes.iter_mut().find(|(p, _, _)| p.coplanar(&f.plane, tol, 1e-9)) {
                    Some((_, fa, fb)) => {
                        if is_a {
                            *fa = true
                        } else {
                            *fb = true
                        }
                    }
                    None => planes.push((f.plane, is_a, !is_a)),
                }
            }
        }
        for (p, ao, bo) in &planes {
            let sa = section(&plate, p, tol).map(|s| s.region).unwrap_or_default();
            let sb = section(&tool, p, tol).map(|s| s.region).unwrap_or_default();
            println!(
                "plane n={:?} o={:?} a={ao} b={bo} sa={:.2} sb={:.2} a\\b={:.2} b^a={:.2}",
                p.normal,
                p.origin,
                sa.area(),
                sb.area(),
                cadcraft_geom::bool2d::difference(&sa, &sb).map(|r| r.area()).unwrap_or(-1.0),
                cadcraft_geom::bool2d::intersect(&sb, &sa).map(|r| r.area()).unwrap_or(-1.0),
            );
        }
        match boolean(&plate, &tool, Bool3::Difference) {
            Ok(s) => println!("result volume {:.4}", s.volume()),
            Err(e) => println!("error {e}"),
        }
    }
}
