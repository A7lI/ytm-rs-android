//! Choose one direction when an Android drag starts over a horizontal shelf.

use egui::{Context, Event, Id, LayerId, Pos2, Rect, TouchDeviceId, TouchId, TouchPhase, Ui};

const SLOP: f32 = 5.0;
const COAST_DECAY: f32 = 0.32;
const MAX_FLING_SPEED: f32 = 6000.0;
const COAST_MIN_SPEED: f32 = 15.0;
const RELEASE_WINDOW: f64 = 0.08;
type Finger = (TouchDeviceId, TouchId);

#[derive(Clone)]
struct Shelf {
    id: Id,
    bounds: Rect,
    layer: LayerId,
}

#[derive(Clone, Copy)]
enum Axis {
    Horizontal,
    Vertical,
}

#[derive(Clone)]
struct Swipe {
    finger: Finger,
    shelf: Id,
    start: Pos2,
    pointer: Pos2,
    axis: Option<Axis>,
    interrupted: bool,
    last_movement: f64,
    velocity: f32,
}

#[derive(Clone, Copy)]
struct Coast {
    shelf: Id,
    velocity: f32,
    time: f64,
}

#[derive(Clone, Default)]
struct Shelves {
    pass: u64,
    regions: Vec<Shelf>,
    fingers: Vec<Finger>,
    swipe: Option<Swipe>,
    delta: Option<(Id, f32)>,
    coast: Option<Coast>,
}

impl Shelves {
    fn touch_at(&mut self, finger: Finger, phase: TouchPhase, pos: Pos2, shelf: Option<Id>, time: f64) -> Option<Pos2> {
        match phase {
            TouchPhase::Start => {
                self.coast = None;
                self.delta = None;
                if !self.fingers.contains(&finger) {
                    self.fingers.push(finger);
                }
                if self.fingers.as_slice() == [finger] {
                    self.swipe = shelf.map(|shelf| Swipe {
                        finger,
                        shelf,
                        start: pos,
                        pointer: pos,
                        axis: None,
                        interrupted: false,
                        last_movement: time,
                        velocity: 0.0,
                    });
                } else if let Some(swipe) = &mut self.swipe {
                    swipe.interrupted = true;
                }
                None
            }
            TouchPhase::Move => {
                let swipe = self.swipe.as_mut().filter(|swipe| swipe.finger == finger)?;
                if swipe.interrupted {
                    return Some(swipe.pointer);
                }
                if swipe.axis.is_none() {
                    let movement = pos - swipe.start;
                    if movement.x.abs().max(movement.y.abs()) < SLOP {
                        return Some(swipe.start);
                    }
                    swipe.axis = Some(if movement.x.abs() > movement.y.abs() {
                        Axis::Horizontal
                    } else {
                        Axis::Vertical
                    });
                }
                match swipe.axis.expect("drag direction chosen") {
                    Axis::Horizontal => {
                        let dx = pos.x - swipe.pointer.x;
                        if dx != 0.0 {
                            self.delta.get_or_insert((swipe.shelf, 0.0)).1 += dx;
                            let elapsed = time - swipe.last_movement;
                            let speed = (dx / elapsed.clamp(1.0 / 120.0, RELEASE_WINDOW) as f32)
                                .clamp(-MAX_FLING_SPEED, MAX_FLING_SPEED);
                            swipe.velocity = if elapsed > RELEASE_WINDOW || swipe.velocity == 0.0 {
                                speed
                            } else {
                                0.55 * swipe.velocity + 0.45 * speed
                            };
                            swipe.last_movement = time;
                        }
                        swipe.pointer.x = pos.x;
                    }
                    Axis::Vertical => swipe.pointer.y = pos.y,
                }
                Some(swipe.pointer)
            }
            TouchPhase::End | TouchPhase::Cancel => {
                if self.swipe.as_ref().is_some_and(|swipe| swipe.finger == finger) {
                    let swipe = self.swipe.take().expect("active finger owns swipe");
                    if phase == TouchPhase::End && !swipe.interrupted
                        && matches!(swipe.axis, Some(Axis::Horizontal))
                        && time - swipe.last_movement <= RELEASE_WINDOW
                        && swipe.velocity.abs() >= COAST_MIN_SPEED
                    {
                        self.coast = Some(Coast { shelf: swipe.shelf, velocity: swipe.velocity, time });
                    }
                }
                self.fingers.retain(|active| *active != finger);
                None
            }
        }
    }

    fn tick(&mut self, time: f64) -> bool {
        let Some(coast) = &mut self.coast else { return false };
        let elapsed = (time - coast.time).max(0.0) as f32;
        let next_velocity = coast.velocity * (-elapsed / COAST_DECAY).exp();
        let distance = (coast.velocity - next_velocity) * COAST_DECAY;
        if distance != 0.0 {
            self.delta.get_or_insert((coast.shelf, 0.0)).1 += distance;
        }
        coast.velocity = next_velocity;
        coast.time = time;
        if next_velocity.abs() < COAST_MIN_SPEED {
            self.coast = None;
        }
        self.coast.is_some()
    }
}

fn state_id() -> Id {
    Id::unique("android-shelf-swipes")
}

/// Record clipped shelves from the latest painted pass.
pub(super) fn register(ui: &Ui, id: Id, rect: Rect) {
    let pass = ui.ctx().cumulative_pass_nr();
    let clipped = rect.intersect(ui.clip_rect());
    if !clipped.is_positive() {
        return;
    }
    ui.ctx().data_mut(|data| {
        let state = data.get_temp_mut_or_default::<Shelves>(state_id());
        if state.pass != pass {
            state.pass = pass;
            state.regions.clear();
        }
        state.regions.push(Shelf { id, bounds: clipped, layer: ui.layer_id() });
    });
}

pub(super) fn input(ctx: &Context, events: &mut [Event], time: f64) {
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|data| data.get_temp_mut_or_default::<Shelves>(state_id()).delta = None);
    let mut projected = None;
    for event in events {
        // egui-winit pairs each primary Touch::Move with a PointerMoved event.
        // The latter drives the parent ScrollArea and must see the same axis.
        let pointer = projected.take();
        match event {
            Event::Touch { device_id, id, phase, pos, .. } => {
                let layer = (*phase == TouchPhase::Start).then(|| ctx.layer_id_at(*pos)).flatten();
                projected = ctx.data_mut(|data| {
                    let state = data.get_temp_mut_or_default::<Shelves>(state_id());
                    let shelf = if *phase == TouchPhase::Start && state.pass + 1 == pass {
                        state.regions.iter().rev()
                            .find(|shelf| Some(shelf.layer) == layer && shelf.bounds.contains(*pos))
                            .map(|shelf| shelf.id)
                    } else {
                        None
                    };
                    state.touch_at((*device_id, *id), *phase, *pos, shelf, time)
                });
            }
            Event::PointerMoved(pos) => {
                if let Some(pointer) = pointer {
                    *pos = pointer;
                }
            }
            _ => {}
        }
    }
    if ctx.data_mut(|data| data.get_temp_mut_or_default::<Shelves>(state_id()).tick(time)) {
        ctx.request_repaint();
    }
}

pub(super) fn take_delta(ctx: &Context, id: Id) -> Option<f32> {
    ctx.data_mut(|data| {
        let state = data.get_temp_mut_or_default::<Shelves>(state_id());
        if state.delta.is_some_and(|(owner, _)| owner == id) {
            state.delta.take().map(|(_, delta)| delta)
        } else {
            None
        }
    })
}

pub(super) fn stop_coast(ctx: &Context, id: Id) {
    ctx.data_mut(|data| {
        let state = data.get_temp_mut_or_default::<Shelves>(state_id());
        if state.coast.is_some_and(|coast| coast.shelf == id) {
            state.coast = None;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::pos2;

    const FINGER: Finger = (TouchDeviceId(1), TouchId(2));

    impl Shelves {
        fn touch(&mut self, finger: Finger, phase: TouchPhase, pos: Pos2, shelf: Option<Id>) -> Option<Pos2> {
            self.touch_at(finger, phase, pos, shelf, 0.0)
        }
    }

    #[test]
    fn horizontal_drag_stays_with_its_shelf_and_coasts_after_release() {
        let mut scroll = Shelves::default();
        let shelf = Id::new("original");
        scroll.touch(FINGER, TouchPhase::Start, pos2(110.0, 260.0), Some(shelf));
        assert_eq!(scroll.touch(FINGER, TouchPhase::Move, pos2(108.0, 258.0), None), Some(pos2(110.0, 260.0)));
        assert_eq!(scroll.touch(FINGER, TouchPhase::Move, pos2(80.0, 254.0), None), Some(pos2(80.0, 260.0)));
        assert_eq!(scroll.touch(FINGER, TouchPhase::Move, pos2(60.0, 96.0), Some(Id::new("other"))), Some(pos2(60.0, 260.0)));
        assert_eq!(scroll.delta.take(), Some((shelf, -50.0)));
        scroll.touch(FINGER, TouchPhase::End, pos2(60.0, 96.0), None);
        assert!(scroll.swipe.is_none() && scroll.delta.is_none());
        assert!(scroll.tick(0.03));
        let first = scroll.delta.take().unwrap();
        assert_eq!(first.0, shelf);
        assert!(first.1 < 0.0);
        assert!(scroll.tick(0.06));
        assert!(scroll.delta.take().unwrap().1.abs() < first.1.abs());
        // A fast fling keeps moving for a little longer, then settles.
        assert!(!scroll.tick(2.0));
    }

    #[test]
    fn held_or_cancelled_swipe_does_not_coast() {
        let shelf = Id::new("shelf");
        for (phase, time) in [(TouchPhase::End, 0.25), (TouchPhase::Cancel, 0.04)] {
            let mut scroll = Shelves::default();
            scroll.touch_at(FINGER, TouchPhase::Start, Pos2::ZERO, Some(shelf), 0.0);
            scroll.touch_at(FINGER, TouchPhase::Move, pos2(30.0, 0.0), None, 0.02);
            scroll.touch_at(FINGER, phase, pos2(30.0, 0.0), None, time);
            assert!(scroll.coast.is_none());
        }
    }

    #[test]
    fn vertical_drag_over_shelf_does_not_switch_axes() {
        let mut scroll = Shelves::default();
        let shelf = Id::new("original");
        scroll.touch(FINGER, TouchPhase::Start, Pos2::ZERO, Some(shelf));
        assert_eq!(scroll.touch(FINGER, TouchPhase::Move, pos2(8.0, 12.0), None), Some(pos2(0.0, 12.0)));
        assert_eq!(scroll.touch(FINGER, TouchPhase::Move, pos2(130.0, 24.0), None), Some(pos2(0.0, 24.0)));
        assert!(scroll.delta.is_none());
        scroll.touch(FINGER, TouchPhase::Cancel, pos2(130.0, 24.0), None);
        scroll.touch(FINGER, TouchPhase::Start, Pos2::ZERO, Some(shelf));
        assert_eq!(scroll.touch(FINGER, TouchPhase::Move, pos2(30.0, 4.0), None), Some(pos2(30.0, 0.0)));
        assert_eq!(scroll.delta, Some((shelf, 30.0)));
    }

    #[test]
    fn tapping_and_multitouch_do_not_scroll_a_shelf() {
        let mut scroll = Shelves::default();
        let other = (TouchDeviceId(1), TouchId(3));
        let shelf = Id::new("shelf");
        scroll.touch(FINGER, TouchPhase::Start, Pos2::ZERO, Some(shelf));
        scroll.touch(FINGER, TouchPhase::End, Pos2::ZERO, None);
        assert!(scroll.delta.is_none());
        scroll.touch(FINGER, TouchPhase::Start, Pos2::ZERO, Some(shelf));
        scroll.touch(other, TouchPhase::Start, pos2(30.0, 20.0), Some(shelf));
        assert_eq!(scroll.touch(FINGER, TouchPhase::Move, pos2(60.0, 80.0), None), Some(Pos2::ZERO));
        scroll.touch(other, TouchPhase::Cancel, pos2(30.0, 20.0), None);
        assert_eq!(scroll.touch(FINGER, TouchPhase::Move, pos2(80.0, 96.0), None), Some(Pos2::ZERO));
        assert!(scroll.delta.is_none());
    }

    #[test]
    fn dragging_outside_shelves_preserves_pointer_input() {
        let mut scroll = Shelves::default();
        scroll.touch(FINGER, TouchPhase::Start, Pos2::ZERO, None);
        assert_eq!(scroll.touch(FINGER, TouchPhase::Move, pos2(18.0, 30.0), None), None);
        assert!(scroll.delta.is_none());
    }
}
