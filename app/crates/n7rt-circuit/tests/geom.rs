//! Pure geometry of contract §1.3 and §3.4: rotations, canonical paths, autoroute candidates.

mod common;

use common::{pt, pts, Rng};
use n7rt_circuit::geom::{autoroute, canonical, is_orthogonal, rotate, rotate_dir};
use n7rt_circuit::{Dir, Pt, Rot};

fn unit(d: Dir) -> Pt {
    match d {
        Dir::L => pt(-1, 0),
        Dir::R => pt(1, 0),
        Dir::U => pt(0, -1),
        Dir::D => pt(0, 1),
    }
}

#[test]
fn rotations_follow_the_contract_formulas() {
    // §1.3: R90 (x, y) → (−y, x), R180 (−x, −y), R270 (y, −x); clockwise with y down.
    assert_eq!(rotate(pt(1, 2), Rot::R0), pt(1, 2));
    assert_eq!(rotate(pt(1, 2), Rot::R90), pt(-2, 1));
    assert_eq!(rotate(pt(1, 2), Rot::R180), pt(-1, -2));
    assert_eq!(rotate(pt(1, 2), Rot::R270), pt(2, -1));
    // Right (1, 0) turns to down (0, 1): clockwise on screen.
    assert_eq!(rotate(pt(1, 0), Rot::R90), pt(0, 1));
    let mut rng = Rng::new(7);
    for _ in 0..200 {
        let p = pt(rng.range(-500, 500), rng.range(-500, 500));
        let four = (0..4).fold(p, |q, _| rotate(q, Rot::R90));
        assert_eq!(four, p);
        assert_eq!(rotate(rotate(p, Rot::R90), Rot::R270), p);
        assert_eq!(rotate(rotate(p, Rot::R90), Rot::R90), rotate(p, Rot::R180));
    }
}

#[test]
fn directions_rotate_like_vectors() {
    for d in [Dir::L, Dir::R, Dir::U, Dir::D] {
        for r in [Rot::R0, Rot::R90, Rot::R180, Rot::R270] {
            assert_eq!(unit(rotate_dir(d, r)), rotate(unit(d), r), "{d:?} {r:?}");
        }
    }
    assert_eq!(rotate_dir(Dir::R, Rot::R90), Dir::D);
    assert_eq!(rotate_dir(Dir::U, Rot::R90), Dir::R);
}

#[test]
fn canonical_drops_zero_segments_collinear_points_and_spikes() {
    assert_eq!(canonical(&pts(&[(0, 0), (0, 0), (3, 0)])), pts(&[(0, 0), (3, 0)]));
    assert_eq!(canonical(&pts(&[(0, 0), (2, 0), (5, 0)])), pts(&[(0, 0), (5, 0)]));
    assert_eq!(canonical(&pts(&[(0, 0), (5, 0), (2, 0), (2, 3)])), pts(&[(0, 0), (2, 0), (2, 3)]));
    assert_eq!(canonical(&pts(&[(0, 0), (0, 5), (0, 2), (4, 2)])), pts(&[(0, 0), (0, 2), (4, 2)]));
    assert_eq!(canonical(&pts(&[(0, 0), (3, 0), (3, 4), (3, 4), (6, 4)])), pts(&[(0, 0), (3, 0), (3, 4), (6, 4)]));
    // A spike that comes back to an end point: both ends are kept.
    assert_eq!(canonical(&pts(&[(0, 0), (4, 0), (0, 0)])), pts(&[(0, 0), (0, 0)]));
    assert_eq!(canonical(&pts(&[(1, 1), (1, 1)])), pts(&[(1, 1), (1, 1)]));
    assert_eq!(canonical(&pts(&[(1, 1)])), pts(&[(1, 1)]));
    assert!(canonical(&[]).is_empty());
    // A detour that cancels out entirely.
    assert_eq!(canonical(&pts(&[(0, 0), (0, 3), (0, 0), (5, 0)])), pts(&[(0, 0), (5, 0)]));
}

#[test]
fn canonical_is_idempotent_and_keeps_the_ends() {
    let mut rng = Rng::new(11);
    for _ in 0..2000 {
        let n = rng.range(2, 8) as usize;
        let mut path = vec![pt(rng.range(-5, 5), rng.range(-5, 5))];
        for _ in 1..n {
            let last = *path.last().unwrap();
            // Orthogonal moves, zero-length steps and reversals included.
            let step = rng.range(-3, 3);
            path.push(if rng.chance(50) { pt(last.x + step, last.y) } else { pt(last.x, last.y + step) });
        }
        let c = canonical(&path);
        assert_eq!(canonical(&c), c);
        assert_eq!(c.first(), path.first());
        assert_eq!(c.last(), path.last());
        if c[0] != c[c.len() - 1] {
            assert!(is_orthogonal(&c), "{path:?} → {c:?}");
            // No interior point is collinear with its neighbours: segments alternate.
            for s in c.windows(3) {
                assert_ne!(s[0].x == s[1].x, s[1].x == s[2].x, "{c:?}");
            }
        }
    }
}

#[test]
fn orthogonality() {
    assert!(is_orthogonal(&pts(&[(0, 0), (3, 0), (3, 4)])));
    assert!(!is_orthogonal(&pts(&[(0, 0), (3, 1)])));
    assert!(!is_orthogonal(&pts(&[(0, 0), (0, 0)])));
    assert!(!is_orthogonal(&pts(&[(0, 0)])));
    assert!(!is_orthogonal(&[]));
}

#[test]
fn autoroute_candidates_in_the_fixed_order() {
    // Facing pins, not aligned: L via (b.x, a.y), L via (a.x, b.y), Z horizontal, Z vertical, U (an S here).
    let c = autoroute(pt(0, 0), Dir::R, pt(10, 4), Dir::L);
    assert_eq!(
        c,
        vec![pts(&[(10, 0)]), pts(&[(0, 4)]), pts(&[(5, 0), (5, 4)]), pts(&[(0, 2), (10, 2)]), pts(&[(2, 0), (2, 2), (8, 2), (8, 4)])]
    );
    // Aligned: the straight wire, every other candidate collapses onto it and is dropped.
    assert_eq!(autoroute(pt(0, 0), Dir::R, pt(6, 0), Dir::L), vec![Vec::<Pt>::new()]);
    // Same lead direction: the U goes beyond the outer stub.
    let c = autoroute(pt(0, 0), Dir::U, pt(6, -3), Dir::U);
    assert_eq!(
        c,
        vec![pts(&[(6, 0)]), pts(&[(0, -3)]), pts(&[(3, 0), (3, -3)]), pts(&[(0, -2), (6, -2)]), pts(&[(0, -5), (6, -5)])]
    );
    // Perpendicular leads: stubs then one corner.
    let c = autoroute(pt(0, 0), Dir::R, pt(5, 5), Dir::U);
    assert_eq!(c.last().unwrap(), &pts(&[(2, 0), (2, 3), (5, 3)]));
    // x_mid = a.x + ⌊(b.x − a.x)/2⌋ rounds towards −∞.
    let c = autoroute(pt(0, 0), Dir::L, pt(-3, 5), Dir::R);
    assert!(c.contains(&pts(&[(-2, 0), (-2, 5)])));
    // Same point: nothing is orthogonal.
    assert!(autoroute(pt(1, 1), Dir::L, pt(1, 1), Dir::R).is_empty());
}

#[test]
fn every_candidate_is_canonical_and_orthogonal() {
    let dirs = [Dir::L, Dir::R, Dir::U, Dir::D];
    let mut rng = Rng::new(3);
    for _ in 0..3000 {
        let a = pt(rng.range(-20, 20), rng.range(-20, 20));
        let b = pt(rng.range(-20, 20), rng.range(-20, 20));
        let (da, db) = (*rng.pick(&dirs).unwrap(), *rng.pick(&dirs).unwrap());
        let c = autoroute(a, da, b, db);
        if a != b {
            assert!(!c.is_empty());
        }
        for bends in &c {
            let mut path = vec![a];
            path.extend(bends);
            path.push(b);
            assert!(is_orthogonal(&path));
            assert_eq!(canonical(&path), path);
        }
        for (i, x) in c.iter().enumerate() {
            assert!(!c[i + 1..].contains(x), "duplicate candidate");
        }
    }
}
