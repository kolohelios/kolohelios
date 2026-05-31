use aof::render;

const SAMPLE_SVG: &str = "tests/fixtures/render/sample.svg";

#[test]
fn rasterize_sample_produces_non_empty_image() {
    let bytes = std::fs::read(SAMPLE_SVG).expect("fixture must exist");
    let img = render::rasterize(&bytes).expect("rasterize must succeed");

    let (w, h) = (img.width(), img.height());
    assert_eq!(w, 120, "fixture width comes from the SVG viewBox");
    assert_eq!(h, 60, "fixture height comes from the SVG viewBox");

    // Verify the rasterizer actually drew something: the blue circle in
    // the fixture should contribute at least one fully-opaque pixel.
    let rgba = img.to_rgba8();
    let has_opaque = rgba.pixels().any(|p| p.0[3] == 255);
    assert!(has_opaque, "rasterized image should have opaque pixels");
}

#[test]
fn rasterize_invalid_svg_returns_error() {
    let bytes = b"not an svg";
    let result = render::rasterize(bytes);
    assert!(result.is_err(), "garbage input must fail SVG parse");
}
