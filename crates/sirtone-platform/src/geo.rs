//! «Определить автоматически» (T5.5, FR-CTX-002, D-54): the Windows location service
//! (`Windows.Devices.Geolocation.Geolocator`), coarse accuracy. Called only when the person
//! presses the button; access is the person's choice in Windows settings — denied or off is an
//! error, never a change of settings from here.

use windows::Devices::Geolocation::{GeolocationAccessStatus, Geolocator, PositionAccuracy};

/// (latitude, longitude) or why not.
pub fn locate() -> Result<(f64, f64), String> {
    let access = Geolocator::RequestAccessAsync()
        .and_then(|op| op.join())
        .map_err(|e| e.message())?;
    if access != GeolocationAccessStatus::Allowed {
        return Err("location access is off in Windows".into());
    }
    let g = Geolocator::new().map_err(|e| e.message())?;
    let _ = g.SetDesiredAccuracy(PositionAccuracy::Default);
    let pos = g
        .GetGeopositionAsync()
        .and_then(|op| op.join())
        .map_err(|e| e.message())?;
    let p = pos
        .Coordinate()
        .and_then(|c| c.Point())
        .and_then(|p| p.Position())
        .map_err(|e| e.message())?;
    Ok((p.Latitude, p.Longitude))
}
