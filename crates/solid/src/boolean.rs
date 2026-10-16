//! Boolean combination of planar-faced solids.
//!
//! # The algorithm in one paragraph
//!
//! Collect every distinct plane that carries a face of either solid. For each such plane, cut
//! *both* solids with it — that gives the material each one occupies in that plane — and combine
//! those two 2D regions with the planar boolean. Whatever is left becomes one face per region.
//!
//! # Why that shape of algorithm
//!
//! - **Coplanar faces fall out for free.** Two boxes sharing a face plane produce the face they
//!   agree on, not two that then have to be stitched together afterwards, and no sliver survives
//!   along the seam. Two bodies on *opposite* sides of a shared plane are the interesting case:
//!   neither side's face survives whole, so each contributes the part the other does not cover, and
//!   the two parts are two faces — one cannot carry both, because they point opposite ways.
//! - **It is symmetric, and it is made to be.** For union and intersection the plane list and the
//!   two sections are the same set whichever way round the operands go, but the *frames* they are
//!   expressed in are not: each shared plane takes its frame from whichever operand's face was read
//!   first. The operands are therefore put in a canonical order from their own geometry before
//!   anything else happens, so `a ∪ b` and `b ∪ a` are the same run over the same pair rather than
//!   two runs that happen to agree. `a - b` is not commutative and is left alone.
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
///
/// `Union` and `Intersect` are commutative, so the two calls must be the *same* run over the same
/// two solids — not merely the same arithmetic but the same arithmetic in the same order, because
/// that is what the kernel's properties compare. The operands are therefore ordered by a key read
/// off their geometry before anything else happens; see [`operand_key`].
pub fn boolean(a: &Solid, b: &Solid, op: Bool3) -> Result<Solid, SolidError> {
    // `a u b` and `b u a` are the same solid, so they run the same code over the same pair. Which of
    // the two the caller wrote first must not reach any decision: the plane list is built from one
    // operand's faces first and takes each shared plane's *frame* from whichever came first, the
    // planar booleans are handed the sections in a fixed order, and the weld and the orientation
    // pass both walk the faces in build order. None of that is symmetric by itself — a face carried
    // across a shared plane, or two coplanar faces merged, comes out with a loop wound the other way
    // depending on which operand's normal became the result's normal — so the order is fixed once,
    // here, from geometry the caller did not choose.
    //
    // `Difference` is not commutative and keeps the caller's order.
    let (a, b) = if matches!(op, Bool3::Union | Bool3::Intersect) && operand_key(b) < operand_key(a) { (b, a) } else { (a, b) };
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
            // Two solids on *opposite* sides of a shared plane: that plane is interior only where
            // they overlap. Everywhere else it is still the boundary of one of them, so the face is
            // exactly what one of them covers and the other does not. Dropping the plane outright
            // (which is what this used to do) is right only when the two sections coincide, and
            // leaves an open shell the moment a box is stacked on a *part* of another — the case
            // `a_union_across_a_shared_plane_keeps_the_exposed_parts` pins down.
            (Bool3::Union, true, true, false) => bool2d::difference(&sa, &sb),
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
        // One face per region, never one face for the whole set.
        //
        // Two regions of the same plane that only touch along an edge — the section of a stack of
        // two blocks stepped over each other is exactly that, on every side plane — are two faces,
        // not one: a single face carrying both uses that shared edge twice, once from each of its
        // loops, so every corner along the join has four boundary edges where a manifold vertex has
        // two, and the vertex fan check reads the solid as pinched. The two parts are coplanar, so
        // nothing downstream cares that they are not one face.
        faces.extend(faces_from(*p, region, false)?);
        // Union across a plane the two operands sit on *opposite* sides of: the exposed part of
        // each operand's face is a boundary of the union, but the two parts point in opposite
        // directions — A's top face looks up, B's bottom face looks down. A planar face carries
        // exactly one normal, so they cannot be one face, and merging them into one is what left
        // an open shell along the join.
        //
        // The handover is `Face::flipped`, which carries the boundary across the mirrored frame
        // *and* winds it back. Handing over the coordinates alone — `(x, y) -> (-x, y)`, which is
        // exactly what `Plane::flipped` does to `u` — leaves the loop reading clockwise about its
        // own normal. Nothing downstream can repair that: `orient_faces` probes the normal and never
        // sees the loop, so the edge it shares with the face next to it ends up traversed twice in
        // the same direction and the union refuses with `InconsistentFaces`.
        if matches!((op, a_owns, b_owns, same_side), (Bool3::Union, true, true, false))
            && let Some(back) = bool2d::difference(&sb, &sa)
            && !back.is_empty()
        {
            faces.extend(faces_from(*p, back, true)?);
        }
    }
    let mut solid = Solid::from_faces(faces)?;
    // Faces that a tool only *touched* keep their whole loop, while the faces around the touch are
    // split there. Weld before validating, or the shell reads as having an edge used once.
    crate::solid::weld_edges(&mut solid.faces, tol);
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
    fn union_and_intersection_do_not_depend_on_the_operand_order() {
        // Not merely "both give the same volume": the two calls are the *same* run over the same two
        // solids, so they agree on every face and every loop, and they refuse or succeed together.
        // The pairs here are the shapes that used to disagree — two blocks stacked across a shared
        // plane, whose footprint does not line up.
        let pairs = [
            (box_at(Vec3::ZERO, Vec3::new(2.0, 2.0, 1.0)), box_at(Vec3::new(1.0, 0.0, 1.0), Vec3::new(3.0, 2.0, 2.0))),
            (box_at(Vec3::ZERO, Vec3::new(2.0, 2.0, 1.0)), box_at(Vec3::new(1.0, -1.0, 1.0), Vec3::new(4.0, 3.0, 3.0))),
            (box_at(Vec3::ZERO, Vec3::new(4.0, 4.0, 1.0)), box_at(Vec3::new(1.0, 1.0, 1.0), Vec3::new(3.0, 3.0, 2.0))),
            // A corner-to-corner touch: no shared material at all, so the union is two lumps joined
            // at a point — refused by `validate_vertices` today, but the two orders must agree about
            // that too, which is what this asserts.
            (box_at(Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0)), box_at(Vec3::new(1.0, 1.0, 1.0), Vec3::new(2.0, 2.0, 2.0))),
            // Same base plane, same side: the aligned stack, where the two footprints coincide.
            (box_at(Vec3::ZERO, Vec3::new(2.0, 2.0, 1.0)), box_at(Vec3::ZERO, Vec3::new(2.0, 2.0, 3.0))),
        ];
        for (a, b) in &pairs {
            for op in [Bool3::Union, Bool3::Intersect] {
                let (ab, ba) = (boolean(a, b, op), boolean(b, a, op));
                match (ab, ba) {
                    (Ok(x), Ok(y)) => {
                        assert_eq!(x.faces.len(), y.faces.len(), "{op:?}: {} faces one way, {} the other", x.faces.len(), y.faces.len());
                        assert_eq!(x.faces, y.faces, "{op:?}: the two runs disagree about a loop");
                        assert_eq!(x.volume().to_bits(), y.volume().to_bits(), "{op:?}: {} vs {}", x.volume(), y.volume());
                    }
                    (Err(e), Err(f)) => assert_eq!(e, f, "{op:?}: one order refused with {e} and the other with {f}"),
                    (Ok(s), Err(e)) => panic!("{op:?}: union one way gave {} faces and the other refused with {e}", s.faces.len()),
                    (Err(e), Ok(s)) => panic!("{op:?}: union the other way gave {} faces and this one refused with {e}", s.faces.len()),
                }
            }
        }
    }

    #[test]
    fn a_union_across_a_shared_plane_keeps_both_exposed_parts() {
        // Two blocks touching across `z = 1` on opposite sides, overlapping over only part of the
        // plane. The union's boundary along that plane is two rectangles — A's exposed part looking
        // up, B's exposed part looking down — and the side planes are two coplanar rectangles each.
        let a = box_at(Vec3::ZERO, Vec3::new(2.0, 2.0, 1.0));
        let b = box_at(Vec3::new(1.0, 0.0, 1.0), Vec3::new(3.0, 2.0, 2.0));
        let u = must(boolean(&a, &b, Bool3::Union), "union of two blocks sharing half a face");
        u.validate().unwrap_or_else(|e| panic!("{e}"));
        u.validate_vertices().unwrap_or_else(|e| panic!("{e}"));
        // Two 2x2x1 blocks whose shared face only half overlaps enclose 4 + 4 and nothing to subtract.
        // (The bounding box is 3x2x2 = 12, but the two missing 1x2x1 corners are not material.)
        assert!((u.volume() - 8.0).abs() < 1e-9, "volume {}", u.volume());
        assert_eq!(u.faces.len(), 10, "an L-shaped prism has 10 faces");
        // Two faces on z = 1, one looking each way, each covering exactly the part the other misses.
        let on_plane: Vec<(Vec3, f64)> =
            u.faces.iter().filter(|f| f.plane.distance(Vec3::new(1.5, 1.0, 1.0)).abs() < 1e-12).map(|f| (f.plane.normal, f.area())).collect();
        assert_eq!(on_plane.len(), 2, "A's exposed part and B's exposed part: {on_plane:?}");
        assert!(on_plane.iter().any(|(n, _)| n.near(Vec3::Z, 1e-12)), "one looks up: {on_plane:?}");
        assert!(on_plane.iter().any(|(n, _)| n.near(Vec3::Z * -1.0, 1e-12)), "and the other looks down: {on_plane:?}");
        for (_, area) in &on_plane {
            assert!((area - 2.0).abs() < 1e-9, "each exposed part is 1x2, got {area}");
        }
        // The step is still a step: the overhang is there and the step down is not.
        assert!(u.contains(Vec3::new(0.5, 1.0, 0.5)), "the lower block");
        assert!(u.contains(Vec3::new(2.5, 1.0, 1.5)), "the overhang");
        assert!(!u.contains(Vec3::new(2.5, 1.0, 0.5)), "and nothing under the overhang");
        assert!(!u.contains(Vec3::new(0.5, 1.0, 1.5)), "nor above the step");
    }

    #[test]
    fn union_across_a_shared_plane_succeeds_in_both_orders_and_agrees() {
        // The defect-1 repro, both ways round. `a ∪ b` and `b ∪ a` are the same run over the same
        // two solids — the operands are put in a canonical order from their own geometry before
        // anything else happens — so they succeed together and agree on every face and every loop.
        let a = box_at(Vec3::ZERO, Vec3::new(2.0, 2.0, 1.0));
        let b = box_at(Vec3::new(1.0, 0.0, 1.0), Vec3::new(3.0, 2.0, 2.0));
        let (Ok(x), Ok(y)) = (boolean(&a, &b, Bool3::Union), boolean(&b, &a, Bool3::Union)) else {
            panic!("the union of two blocks sharing half a face must succeed in both orders");
        };
        assert_eq!(x.faces, y.faces, "the two orders disagree about a loop");
        assert_eq!(x.volume().to_bits(), y.volume().to_bits(), "the two orders disagree about the volume");
        assert_eq!(x.faces.len(), 10, "an L-shaped prism has 10 faces");
    }

    #[test]
    fn a_aligned_stack_across_a_shared_plane_is_still_one_prism() {
        // The control case for the test above: the two footprints coincide, so neither side's face
        // is exposed at all and the shared plane is interior — the union is a plain 2x2x2 prism.
        let a = box_at(Vec3::ZERO, Vec3::new(2.0, 2.0, 1.0));
        let b = box_at(Vec3::new(0.0, 0.0, 1.0), Vec3::new(2.0, 2.0, 2.0));
        let u = must(boolean(&a, &b, Bool3::Union), "union of two exactly stacked blocks");
        u.validate().unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(u.faces.len(), 6, "a prism");
        assert!((u.volume() - 8.0).abs() < 1e-9, "volume {}", u.volume());
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
    fn booleans_with_an_empty_operand_follow_the_algebra() {
        // Subtracting nothing must leave everything. It once returned the empty result instead,
        // which is the sort of bug that erases a drawing without ever saying so.
        let a = box_at(Vec3::ZERO, Vec3::new(4.0, 3.0, 2.0));
        let empty = Solid::default();
        let d = must(boolean(&a, &empty, Bool3::Difference), "difference with nothing");
        assert!((d.volume() - a.volume()).abs() < 1e-9, "volume {}", d.volume());
        let u = must(boolean(&a, &empty, Bool3::Union), "union with nothing");
        assert!((u.volume() - a.volume()).abs() < 1e-9, "volume {}", u.volume());
        assert!(boolean(&a, &empty, Bool3::Intersect).is_err(), "there is nothing to intersect with");
        assert!(boolean(&empty, &a, Bool3::Intersect).is_err(), "and nothing the other way either");
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

/// A total order on two operands that never reads the argument order.
///
/// Built from the bounds and the face count alone, so it is the same number on every run of the
/// same two solids, and it is total — two distinct operands always get distinct orders, so exactly
/// one of `(a, b)` and `(b, a)` survives [`boolean`]'s swap. Bit patterns rather than the floats
/// themselves: `f64`'s own total ordering would work on finite values, but bits are total even for
/// the non-finite bounds hostile input can produce, and a kernel that hangs on the sign of a NaN is
/// not one anybody can debug.
fn operand_key(s: &Solid) -> ([u64; 6], usize) {
    let bb = s.bounds();
    let corners = [bb.min.x, bb.min.y, bb.min.z, bb.max.x, bb.max.y, bb.max.z].map(f64::to_bits);
    (corners, s.faces.len())
}

/// One face per region of a planar result, optionally seen from the other side.
///
/// `flip` is the far side of a plane the two operands sat on *opposite* sides of: the boundary is
/// carried across the mirrored frame and wound back to match the new normal. See the call site for
/// why a coordinate-only handover is not enough.
fn faces_from(plane: Plane, region: Regions, flip: bool) -> Result<Vec<Face>, SolidError> {
    let mut out = Vec::with_capacity(region.len());
    for r in region.iter() {
        let face = Face::new(plane, Regions::new(vec![r.clone()]))?;
        out.push(if flip { face.flipped()? } else { face });
    }
    Ok(out)
}
