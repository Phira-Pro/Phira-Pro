//! Exact finite-domain evaluation of the R8 subtract cameras.
//! No SIMD, GL extension, approximation, or extra image quantization.
use std::sync::OnceLock;

#[derive(Clone, Copy, Default)]
struct Subtract {
    product: f32,
    green: u8,
}

fn unorm(value: f32) -> u8 {
    (value.clamp(0., 1.) * 255.).round() as u8
}

fn reference_subtract(red: u8, green: u8) -> (f32, f32) {
    let g = green as f32 / 255.;
    let t = ((g - 0.2) * -10.).clamp(0., 1.);
    let r = u8::from(enabled(red)) as f32 + t * t * (3. - 2. * t);
    (unorm(r) as f32 / 255., unorm(g * r * 10.) as f32 / 255.)
}

fn table() -> &'static [Subtract] {
    static TABLE: OnceLock<Box<[Subtract]>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut entries = vec![Subtract::default(); 256 * 256];
        for red in 0..=255_u8 {
            for green in 0..=255_u8 {
                let (r, g) = reference_subtract(red, green);
                entries[(red as usize) * 256 + green as usize] = Subtract {
                    // The product is not quantized. Keep the original f32
                    // multiplication before the final normal-camera subtract.
                    product: r * g,
                    green: unorm(g),
                };
            }
        }
        entries.into_boxed_slice()
    })
}

pub(super) fn prepare() { let _ = table(); }

pub(super) fn enabled(red: u8) -> bool {
    // Exhaustively checked against the original f32 [0.09, 0.12) test.
    (23..=30).contains(&red)
}

pub(super) fn disabled(normal: u8, red: u8, green: u8) -> u8 {
    if green == 0 {
        // The subtract product is zero for every red input in this case.
        return normal;
    }
    let product = table()[(red as usize) * 256 + green as usize].product;
    unorm((product - normal as f32 / 255.).abs())
}

pub(super) fn ready(red: u8, green: u8) -> u8 {
    if green == 0 { 0 } else { table()[(red as usize) * 256 + green as usize].green }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Independent copy of the original shader-equivalent arithmetic, including
    // both intermediate R8 roundings and the final output quantization.
    fn original(red: u8, green: u8) -> (f32, f32) {
        let g = green as f32 / 255.;
        let t = ((g - 0.2) * -10.).clamp(0., 1.);
        let r = if (0.09..0.12).contains(&(red as f32 / 255.)) { 1. } else { 0. }
            + t * t * (3. - 2. * t);
        (unorm(r) as f32 / 255., unorm(g * r * 10.) as f32 / 255.)
    }

    #[test]
    fn exact_for_all_r8_camera_inputs() {
        for red in 0..=255_u8 {
            assert_eq!(enabled(red), (0.09..0.12).contains(&(red as f32 / 255.)));
            for green in 0..=255_u8 {
                let (r, g) = original(red, green);
                assert_eq!(ready(red, green), if green == 0 { 0 } else { unorm(g) });
                for normal in 0..=255_u8 {
                    let expected = if normal == 0 && red == 0 && green == 0 { 0 }
                        else { unorm((r * g - normal as f32 / 255.).abs()) };
                    assert_eq!(disabled(normal, red, green), expected,
                        "normal={normal}, red={red}, green={green}");
                }
            }
        }
    }
}
