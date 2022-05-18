//! Helper module for working with DICOM and imaging data.

use std::borrow::Cow;

use dicom::{
    dictionary_std::tags,
    object::{file::ReadPreamble, DefaultDicomObject, OpenFileOptions}, core::DicomValue,
};
use snafu::prelude::*;
use wasm_bindgen::{Clamped, JsValue};
use web_sys::ImageData;

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(whatever, display("{}", message))]
    Other {
        message: String,
        #[snafu(source(from(Box<dyn std::error::Error>, Some)))]
        source: Option<Box<dyn std::error::Error>>,
    },
    #[snafu(display("{:?}", value))]
    Js { value: JsValue },
}

impl From<Error> for JsValue {
    fn from(e: Error) -> Self {
        match e {
            Error::Other { message, .. } => JsValue::from_str(&message),
            Error::Js { value } => value,
        }
    }
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// A set of visualization window level parameters
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct WindowLevel {
    pub width: f64,
    pub center: f64,
}

#[inline]
pub fn byte_data_to_dicom_obj(byte_data: &[u8]) -> Result<dicom::object::DefaultDicomObject> {
    OpenFileOptions::new()
        .read_preamble(ReadPreamble::Always)
        .from_reader(byte_data)
        .whatever_context("Failed to read DICOM data")
}

pub fn window_level_of(obj: &DefaultDicomObject) -> Result<Option<WindowLevel>> {
    let ww = obj
        .element_opt(tags::WINDOW_WIDTH)
        .whatever_context("Could not get attribute WindowWidth")?;

    let wc = obj
        .element_opt(tags::WINDOW_CENTER)
        .whatever_context("Could not get attribute WindowCenter")?;

    match (ww, wc) {
        (Some(ww), Some(wc)) => {
            let ww = ww
                .to_float64()
                .whatever_context("Could not read WindowWidth as a number")?;
            let wc = wc
                .to_float64()
                .whatever_context("Could not read WindowCenter as a number")?;

            Ok(Some(WindowLevel {
                width: ww,
                center: wc,
            }))
        }
        _ => Ok(None),
    }
}

pub fn obj_to_imagedata(obj: &DefaultDicomObject, y_samples: &mut Vec<u8>, lut: &mut Option<Vec<u8>>) -> Result<ImageData> {
    let photometric_interpretation = obj
        .element(tags::PHOTOMETRIC_INTERPRETATION)
        .whatever_context("Could not fetch PhotometricInterpretation")?
        .to_str()
        .whatever_context("Could not read PhotometricInterpretation as a string")?;

    let width = obj
        .element(tags::COLUMNS)
        .whatever_context("Could not fetch Columns")?
        .to_int::<u32>()
        .whatever_context("Columns is not an integer")?;
    let height = obj
        .element(tags::ROWS)
        .whatever_context("Could not fetch Rows")?
        .to_int::<u32>()
        .whatever_context("Rows is not an integer")?;

    match photometric_interpretation.as_ref() {
        "MONOCHROME1" => {
            if lut.is_none() {
                gloo_console::debug!("Creating monochrome2 LUT");
                *lut = Some(simple_pixel_data_lut(obj)?);
            }

            let lut = lut.as_ref().unwrap().as_ref();
            convert_monochrome_to_y_samples(y_samples, obj, Monochrome::Monochrome1, lut)?;
        }
        "MONOCHROME2" => {
            if lut.is_none() {
                gloo_console::debug!("Creating monochrome2 LUT");
                *lut = Some(simple_pixel_data_lut(obj)?);
            }

            let lut = lut.as_ref().unwrap().as_ref();
            convert_monochrome_to_y_samples(y_samples, obj, Monochrome::Monochrome2, lut)?;
        }
        "RGB" => return convert_rgb_to_imagedata(obj, width, height),
        pi => whatever!("Unsupported photometric interpretation {}, sorry. :(", pi),
    }

    ImageData::new_with_u8_clamped_array_and_sh(Clamped(y_samples), width, height)
        .map_err(|value| Error::Js { value })
}

/// create a simple LUT which maps a 16-bit image
pub fn simple_pixel_data_lut(obj: &DefaultDicomObject) -> Result<Vec<u8>> {
    let window_level = window_level_of(obj)?.whatever_context("The given image does not provide window levels :(")?;
    simple_pixel_data_lut_with(obj, window_level)
}
/// create a simple LUT which maps a 16-bit image
/// using the given window level
pub fn simple_pixel_data_lut_with(
    obj: &DefaultDicomObject,
    window_level: WindowLevel,
) -> Result<Vec<u8>> {
    let bits_stored = obj
        .element(tags::BITS_STORED)
        .whatever_context("Could not fetch BitsStored")?
        .to_int::<u16>()
        .whatever_context("BitsStored is not a number")?;

    let mut lut = vec![0; 1 << bits_stored];

    update_pixel_data_lut_with(&mut lut, obj, window_level)?;

    Ok(lut)
}

/// create a simple LUT which maps a 16-bit image
/// using the given window level parameters.
/// 
/// `lut` must have the correct size for the given object
/// (`1 << bits_stored`).
pub fn update_pixel_data_lut_with(
    lut: &mut [u8],
    obj: &DefaultDicomObject,
    window_level: WindowLevel,
) -> Result<()> {
    debug_assert!(lut.len() >= 256);

    let rescale_slope = if let Some(elem) = obj
        .element_opt(tags::RESCALE_SLOPE)
        .whatever_context("Could not fetch RescaleSlope")?
    {
        elem.to_float64()
            .whatever_context("RescaleSlope is not a number")?
    } else {
        1.0
    };

    let rescale_intercept = if let Some(elem) = obj
        .element_opt(tags::RESCALE_INTERCEPT)
        .whatever_context("Could not fetch RescaleSlope")?
    {
        elem.to_float64()
            .whatever_context("RescaleSlope is not a number")?
    } else {
        0.0
    };

    let voi_lut_function = if let Some(elem) = obj
        .element_opt(tags::VOILUT_FUNCTION)
        .whatever_context("Could not fetch VOILUTFunction")?
    {
        elem.to_str()
            .whatever_context("VOILUTFunction is not a string")?
            .to_string()
    } else {
        "LINEAR".to_string()
    };

    if voi_lut_function != "LINEAR"
        && voi_lut_function != "LINEAR_EXACT"
        && voi_lut_function != "SIGMOID"
    {
        whatever!("Unsupported VOI LUT function {}", &voi_lut_function);
    }

    for (i, y) in lut.iter_mut().enumerate() {
        let x = i as f64;
        // rescale
        let x = x * rescale_slope + rescale_intercept;
        // window
        let x = apply_window_level(x, &voi_lut_function, window_level);
        *y = x as u8;
    }

    Ok(())
}

fn apply_window_level(x: f64, voi_lut_function: &str, window_level: WindowLevel) -> f64 {
    let WindowLevel {
        width: ww,
        center: wc,
    } = window_level;

    match voi_lut_function {
        "LINEAR_EXACT" => window_level_linear_exact(x, ww, wc),
        "SIGMOID" => window_level_sigmoid(x, ww, wc),
        "LINEAR" => window_level_linear(x, ww, wc),
        _ => panic!("Unsupported VOI LUT function {}", voi_lut_function),
    }
}

fn window_level_linear(x: f64, ww: f64, wc: f64) -> f64 {
    debug_assert!(ww >= 1.);

    // C.11.2.1.2.1
    let min = wc - (ww - 1.) / 2.;
    let max = wc - 0.5 + (ww - 1.) / 2.;

    if x <= min {
        // if (x <= c - (w-1) / 2), then y = ymin
        0.
    } else if x > max {
        // else if (x > c - 0.5 + (w-1) /2), then y = ymax
        255.
    } else {
        // else y = ((x - (c - 0.5)) / (w-1) + 0.5) * (ymax- ymin) + ymin
        ((x - (wc - 0.5)) / (ww - 1.) + 0.5) * 255.
    }
}

fn window_level_linear_exact(value: f64, ww: f64, wc: f64) -> f64 {
    debug_assert!(ww >= 0.);

    // C.11.2.1.3.2

    let min = wc - ww / 2.;
    let max = wc + ww / 2.;

    if value <= min {
        // if (x <= c - w/2), then y = ymin
        0.
    } else if value > max {
        // else if (x > c + w/2), then y = ymax
        255.
    } else {
        // else y = ((x - c) / w + 0.5) * (ymax - ymin) + ymin
        ((value - wc) / ww + 0.5) * 255.
    }
}

fn window_level_sigmoid(value: f64, ww: f64, wc: f64) -> f64 {
    assert!(ww >= 1.);

    // C.11.2.1.3.1

    255. / (1. + f64::exp(-4. * (value - wc) / ww))
}

#[derive(Debug, Copy, Clone, Eq, Hash, PartialEq)]
pub enum Monochrome {
    Monochrome1,
    Monochrome2,
}

pub fn convert_monochrome_to_y_samples(
    y_samples: &mut Vec<u8>,
    obj: &DefaultDicomObject,
    monochrome: Monochrome,
    lut: &[u8],
) -> Result<()> {

    let bits_allocated = obj
        .element(tags::BITS_ALLOCATED)
        .whatever_context("Could not fetch BitsAllocated")?
        .to_int::<u16>()
        .whatever_context("BitsAllocated is not a number")?;


    match bits_allocated {
        8 => {
            let samples = obj
                .element(tags::PIXEL_DATA)
                .whatever_context("Could not fetch PixelData")?;

            if matches!(samples.value(), DicomValue::PixelSequence { .. }) {
                whatever!("Encapsulated pixel data encoding is not supported at the moment, sorry. :(");
            }

            let samples = samples
                .to_bytes()
                .whatever_context("Could not read PixelData as a sequence of 8-bit integers")?;
    
            if samples.len() * 4 != y_samples.len() {
                y_samples.resize(samples.len() * 4, 255);
            }

            samples
                .iter()
                .map(|x| lut[*x as usize])
                .map(|v| {
                    if monochrome == Monochrome::Monochrome1 {
                        0xFF - v
                    } else {
                        v
                    }
                })
                .zip(y_samples.chunks_mut(4))
                .for_each(|(x, y)| {
                    y[0] = x;
                    y[1] = x;
                    y[2] = x;
                    y[3] = 255;
                });
        }
        16 => {
            let samples = obj
                .element(tags::PIXEL_DATA)
                .whatever_context("Could not fetch PixelData")?;

            if matches!(samples.value(), DicomValue::PixelSequence { .. }) {
                whatever!("Encapsulated pixel data encoding is not supported at the moment, sorry. :(");
            }

            let samples: Cow<[u16]> = samples
                .uint16_slice()
                .map(Cow::from)
                .or_else(|_| {
                    samples
                        .to_multi_int::<u16>()
                        .map(Cow::Owned)
                })
                .whatever_context("Could not read PixelData as a sequence of 16-bit integers")?;

            if samples.len() * 4 != y_samples.len() {
                y_samples.resize(samples.len() * 4, 255);
            }
        
            for (y, x) in y_samples.chunks_mut(4).zip(samples.iter().copied()) {
                let x = lut[x as usize];

                let x = if monochrome == Monochrome::Monochrome1 {
                    0xFF - x
                } else {
                    x
                };

                y[3] = 255;
                y[0] = x;
                y[1] = x;
                y[2] = x;
            }
        }
        _ => {
            whatever!("Unsupported BitsAllocated {} :(", bits_allocated);
        }
    };

    Ok(())
}

pub fn convert_rgb_to_imagedata(
    obj: &DefaultDicomObject,
    width: u32,
    height: u32,
) -> Result<ImageData> {
    let samples_per_pixel = obj
        .element(tags::SAMPLES_PER_PIXEL)
        .whatever_context("Could not fetch SamplesPerPixel")?
        .to_int::<u16>()
        .whatever_context("SamplesPerPixel is not an integer")?;

    if samples_per_pixel != 3 {
        whatever!("Expected 3 samples per pixel, got {}", samples_per_pixel);
    }

    let samples = obj
        .element(tags::PIXEL_DATA)
        .whatever_context("Could not fetch PixelData")?
        .to_bytes()
        .whatever_context("Could not read the bytes of PixelData")?;

    let data: Vec<u8> = samples
        .chunks(3)
        .map(|chunk| <[u8; 3]>::try_from(chunk).unwrap())
        .flat_map(|[r, g, b]| [r, g, b, 0xFF])
        .collect();

    ImageData::new_with_u8_clamped_array_and_sh(Clamped(&data), width, height)
        .map_err(|value| Error::Js { value })
}
