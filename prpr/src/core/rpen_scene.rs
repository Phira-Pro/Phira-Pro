//! RPEN global tracks and camera geometry. Independent of scene navigation and app HUD.
use super::{BpmList, Matrix};
use crate::parse::rpen::{number, Track};
use anyhow::{ensure, Result};
use macroquad::prelude::*;
use serde_json::Value;

/// Preserve the planar homography's W and enforce the protocol's 1..150000 depth.
pub fn clip_model(matrix: &Matrix) -> Mat4 {
    let mut model = crate::ext::nalgebra_to_glm(matrix);
    let projection = unsafe { get_internal_gl() }.quad_gl.get_projection_matrix();
    let zz = projection.z_axis.z;
    if zz.abs() > 1e-9 {
        let a = (150000_f32 + 1.) / (150000. - 1.);
        let b = -2. * 150000. / (150000. - 1.) / 1000.;
        let k = (a - projection.w_axis.z) / zz;
        model.x_axis.z = matrix[(2, 0)] * k;
        model.y_axis.z = matrix[(2, 1)] * k;
        model.w_axis.z = matrix[(2, 2)] * k + b / zz;
    }
    model
}

pub struct Scene {
    pub global_events: Value,
    pub camera: Option<PlaneCamera>,
}
pub struct PlaneCamera {
    axes: Vec<Track>,
    pub follow_chart: bool,
}
impl Scene {
    pub fn new(raw: &Value, bpm: &mut BpmList) -> Result<Self> {
        let events = raw.get("globalEvents").cloned().unwrap_or(Value::Null);
        if !events.is_null() {
            ensure!(events["version"] == 1, "unsupported globalEvents version");
        }
        let types = &events["types"];
        let mut camera = false;
        let mut axes = Vec::new();
        for (kind, id, default) in [
            ("position", "x", 0.),
            ("position", "y", 0.),
            ("position", "z", 1000.),
            ("rotation", "yaw", 0.),
            ("rotation", "pitch", 0.),
            ("rotation", "roll", 0.),
            ("lens", "fov", 2. * (450_f64 / 1000.).atan().to_degrees()),
        ] {
            let key = format!("spatial-camera/{kind}");
            camera |= types.get(&key).is_some();
            let events = types[&key]["columns"][id]["events"].as_array().cloned().unwrap_or_default();
            axes.push(Track::new(&events, bpm, 1., Value::from(default), true)?);
        }
        if let Some(types) = types.as_object() {
            for key in types.keys().filter(|k| !k.starts_with("spatial-camera/")) {
                tracing::warn!(event_type=%key,"RPEN global event has no registered runtime; source data retained");
            }
        }
        let follow_chart = types["spatial-camera/lens"]["settings"]["followChart"].as_bool().unwrap_or(false);
        Ok(Self {
            global_events: events,
            camera: camera.then_some(PlaneCamera { axes, follow_chart }),
        })
    }
}
impl PlaneCamera {
    /// Homography from chart Y-up coordinates into projected Y-up coordinates.
    /// The GPU receives homogeneous W, so geometry is perspective-correct before postprocessing.
    pub fn matrix(&self, time: f64, aspect: f32) -> Matrix {
        let v: Vec<_> = self.axes.iter().map(|t| number(&t.value(time), 0.) as f32).collect();
        let c = vec3(v[0].clamp(-50000., 50000.), v[1].clamp(-50000., 50000.), v[2].clamp(-50000., 50000.));
        let (sy, cy) = v[3].to_radians().sin_cos();
        let (sp, cp) = v[4].clamp(-89., 89.).to_radians().sin_cos();
        let (sr, cr) = v[5].to_radians().sin_cos();
        let right = vec3(cy, 0., sy);
        let up = vec3(-sy * sp, cp, cy * sp);
        let forward = vec3(sy * cp, sp, -cy * cp);
        let (right, up) = (right * cr + up * sr, up * cr - right * sr);
        let f = 450. / (v[6].clamp(10., 150.).to_radians() / 2.).tan();
        let y = 450. * aspect;
        // Normalize W by 1000 to retain the default identity and stable float precision.
        Matrix::new(
            right.x * f / 1000.,
            right.y * y * f / 675. / 1000.,
            -c.dot(right) * f / 675. / 1000.,
            up.x * 675. * f / y / 1000.,
            up.y * f / 1000.,
            -c.dot(up) * f / y / 1000.,
            forward.x * 675. / 1000.,
            forward.y * y / 1000.,
            -c.dot(forward) / 1000.,
        )
    }
    pub fn screen_matrix(&self, time: f64, aspect: f32) -> Matrix {
        let flip = Matrix::new_nonuniform_scaling(&nalgebra::Vector2::new(1., -1.));
        flip * self.matrix(time, aspect) * flip
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn camera_default_identity_and_inverse_after_translation() {
        let mut bpm = BpmList::new(vec![(0., 120.)]);
        let raw = json!({"globalEvents":{"version":1,"types":{"spatial-camera/position":{"columns":{}}}}});
        let s = Scene::new(&raw, &mut bpm).unwrap();
        let m = s.camera.unwrap().matrix(0., 4. / 3.);
        assert!((m - Matrix::identity()).norm() < 1e-6);
        let raw = json!({"globalEvents":{"version":1,"types":{"spatial-camera/position":{"columns":{"x":{"events":[{"startTime":0,"endTime":2,"start":0,"end":675}]}}}}}});
        let s = Scene::new(&raw, &mut bpm).unwrap();
        let m = s.camera.unwrap().matrix(1., 16. / 9.);
        let p = nalgebra::Point2::new(0.3, 0.1);
        let out = m.transform_point(&p);
        assert!((out.x + 0.7).abs() < 1e-5);
        assert!((m.try_inverse().unwrap().transform_point(&out) - p).norm() < 1e-5);
    }
    #[test]
    fn camera_projection_matches_upstream_basis_and_lens() {
        let reference: Value = serde_json::from_str(include_str!("../../tests/fixtures/rpen-extensions/reference.json")).unwrap();
        for case in reference["cameras"].as_array().unwrap() {
            let pose = &case["pose"];
            let mut types = serde_json::Map::new();
            for (kind, ids) in [
                ("position", vec!["x", "y", "z"]),
                ("rotation", vec!["yaw", "pitch", "roll"]),
                ("lens", vec!["fov"]),
            ] {
                let mut columns = serde_json::Map::new();
                for id in ids {
                    columns.insert(id.into(), json!({"events":[{"startTime":0,"endTime":1,"start":pose[id],"end":pose[id]}]}));
                }
                types.insert(format!("spatial-camera/{kind}"), json!({"columns":columns}));
            }
            let mut bpm = BpmList::new(vec![(0., 120.)]);
            let s = Scene::new(&json!({"globalEvents":{"version":1,"types":types}}), &mut bpm).unwrap();
            let m = s.camera.unwrap().matrix(0., number(&case["aspect"], 1.) as f32);
            for sample in case["samples"].as_array().unwrap() {
                let p = nalgebra::Point2::new(number(&sample["x"], 0.) as f32, number(&sample["y"], 0.) as f32);
                let actual = m.transform_point(&p);
                if sample["expected"].is_object() {
                    for (i, k) in ["x", "y"].iter().enumerate() {
                        assert!((actual[i] - number(&sample["expected"][k], 0.) as f32).abs() < 0.00001, "{case}, {sample}, {actual}");
                    }
                }
            }
        }
    }
}
