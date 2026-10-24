use super::*;
fn example() -> geometry::Project {
    serde_json::from_str(include_str!("../examples/outside.json")).unwrap()
}
#[test]
fn analytic_cylinder_length_and_feed() {
    let mut p = example();
    p.features.truncate(1);
    p.features[0].corner.r#type = "none".into();
    p.entry.r#type = "line".into();
    p.exit.r#type = "line".into();
    let a = geometry::analyze(&p, &Cancellation::default()).unwrap();
    assert!(a.valid, "{:?}", a.diagnostics);
    assert_eq!(a.profile.len(), 1);
    assert!((a.profile[0].length() - 18.0).abs() < 1e-10);
    assert_eq!(a.profile[0].start.r, 15.0);
    assert_eq!(a.profile[0].end.z, -18.0);
    assert_eq!(a.segments.len(), 5);
}
#[test]
fn reverse_preserves_analytic_profile_and_feed() {
    let p = example();
    let a = geometry::analyze(&p, &Cancellation::default()).unwrap();
    assert!(a.valid, "{:?}", a.diagnostics);
    let mut reverse = p.clone();
    reverse.reverse = true;
    let b = geometry::analyze(&reverse, &Cancellation::default()).unwrap();
    assert!(b.valid, "{:?}", b.diagnostics);
    assert_eq!(a.profile[0].start, b.profile[0].start);
    let finish: Vec<_> = a.segments.iter().filter(|s| s.role == "finish").collect();
    let flipped: Vec<_> = b
        .segments
        .iter()
        .filter(|s| s.role == "finish")
        .rev()
        .collect();
    for (a, b) in finish.iter().zip(flipped) {
        assert_eq!(a.start, b.end);
        assert_eq!(a.end, b.start);
        assert_eq!(a.feed_per_rev, b.feed_per_rev);
        assert_eq!(a.sweep, -b.sweep);
    }
}
#[test]
fn analytic_quarter_circle_and_tangency() {
    let a = geometry::analyze(&example(), &Cancellation::default()).unwrap();
    let s = a.profile.iter().find(|s| s.center.is_some()).unwrap();
    let center = s.center.unwrap();
    assert!((s.start.sub(center).length() - 2.0).abs() < 1e-10);
    assert!((s.end.sub(center).length() - 2.0).abs() < 1e-10);
    assert!((s.sweep.abs() - std::f64::consts::FRAC_PI_2).abs() < 1e-10);
    assert!((s.length() - std::f64::consts::PI).abs() < 1e-10);
}
#[test]
fn impossible_corner_and_forbidden_boundaries_fail() {
    let mut p = example();
    p.features[0].corner.value = 1000.0;
    assert!(
        !geometry::analyze(&p, &Cancellation::default())
            .unwrap()
            .valid
    );
    let mut p = example();
    p.forbidden.push(geometry::Zone {
        z_min: -19.0,
        z_max: -17.0,
    });
    let a = geometry::analyze(&p, &Cancellation::default()).unwrap();
    assert!(!a.valid);
    assert!(a.diagnostics.iter().any(|d| d.code == "forbidden"));
}
#[test]
fn cancellation_and_duplicate_ids_fail() {
    let c = Cancellation::default();
    c.cancel();
    assert_eq!(
        geometry::analyze(&example(), &c).unwrap_err().code,
        "cancelled"
    );
    let mut p = example();
    p.features[1].id = p.features[0].id.clone();
    assert!(
        !geometry::analyze(&p, &Cancellation::default())
            .unwrap()
            .valid
    );
}
#[test]
fn number_policy_and_output_tampering() {
    assert_eq!(post::number(-0.0000001).unwrap(), "0");
    assert_eq!(post::number(1.2345675).unwrap(), "1.234568");
    assert_eq!(post::number(-1.2345675).unwrap(), "-1.234568");
    assert!(post::number(f64::NAN).is_err());
    let p = example();
    let a = geometry::analyze(&p, &Cancellation::default()).unwrap();
    let model = post::lower(&p, &a).unwrap();
    let mut text = format!(
        "G18 G90 G710 DIAMON\nG95\nG96 S120 LIMS=2500 F{}\n",
        model["blocks"][0]["f"].as_str().unwrap()
    );
    for b in model["blocks"].as_array().unwrap() {
        text += &format!(
            "{} X{} Z{}{} F{}\n",
            b["motion"].as_str().unwrap(),
            b["x"].as_str().unwrap(),
            b["z"].as_str().unwrap(),
            b["cr"]
                .as_str()
                .map(|s| format!(" CR={s}"))
                .unwrap_or_default(),
            b["f"].as_str().unwrap()
        );
    }
    text += "M17\n";
    assert!(post::verify(&text, &model).unwrap().ends_with("M17\r\n"));
    assert!(post::verify(&text.replacen("G2 ", "G3 ", 1), &model).is_err());
    assert!(post::verify(&(text.clone() + "G0 X1 Z0\n"), &model).is_err());
    assert!(post::verify(&text.replace("F0.1", "F0.2"), &model).is_err());
    assert!(post::verify(&text.replace("G95\n", ""), &model).is_err());
    assert!(post::verify(&text.replace("M17", "M17\n; 中文"), &model).is_err());
}
#[test]
fn support_combinations_and_stock() {
    for mode in ["outside", "inside"] {
        for direction in [-1, 1] {
            for reverse in [false, true] {
                let mut p = example();
                p.mode = mode.into();
                p.direction = direction;
                p.reverse = reverse;
                if mode == "inside" {
                    p.start_diameter = 40.0;
                    p.features[1].diameter = Some(30.0);
                    p.features[2].diameter = Some(30.0);
                    p.features[3].diameter = Some(24.0);
                }
                let a = geometry::analyze(&p, &Cancellation::default()).unwrap();
                assert!(a.valid, "{mode} {direction} {reverse}: {:?}", a.diagnostics);
                post::lower(&p, &a).unwrap();
            }
        }
    }
}

#[test]
fn stock_envelope_and_touching_forbidden_zone_are_checked() {
    let mut p = example();
    p.stock = Some(geometry::Stock {
        z_min: -50.0,
        z_max: 0.0,
        outer_diameter: 47.0,
        inner_diameter: 0.0,
    });
    let a = geometry::analyze(&p, &Cancellation::default()).unwrap();
    assert!(a.valid, "{:?}", a.diagnostics);
    p.stock.as_mut().unwrap().outer_diameter = 10.0;
    assert!(
        !geometry::analyze(&p, &Cancellation::default())
            .unwrap()
            .valid
    );
    let mut p = example();
    p.forbidden.push(geometry::Zone {
        z_min: 2.0,
        z_max: 3.0,
    });
    assert!(
        !geometry::analyze(&p, &Cancellation::default())
            .unwrap()
            .valid
    );
}
