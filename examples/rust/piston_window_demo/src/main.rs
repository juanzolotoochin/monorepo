extern crate piston_window;

use piston_window::*;

fn main() {
    let mut window: PistonWindow = WindowSettings::new("Hello Piston!", [640, 480])
        .exit_on_esc(true)
        .build()
        .unwrap();

    while let Some(e) = window.next() {
        window.draw_2d(&e, |c, g, _| {
            clear([1.0; 4], g);
            rectangle(
                [1.0, 0.0, 0.0, 1.0], // Red color
                [50.0, 50.0, 100.0, 100.0], // x, y, w, h
                c.transform,
                g,
            );
        });
    }
}
