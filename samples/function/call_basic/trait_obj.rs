use core::hint::black_box;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;

fn main() {
    let v1 = Counter(1);
    let obj_ref: &dyn Demo = &v1;
    obj_ref.by_ref();

    let mut v2 = Counter(2);
    let obj_mut: &mut dyn Demo = &mut v2;
    obj_mut.by_mut();

    let obj_box: Box<dyn Demo> = Box::new(Counter(3));
    obj_box.by_box();

    let obj_rc: Rc<dyn Demo> = Rc::new(Counter(4));
    obj_rc.by_rc();

    let obj_arc: Arc<dyn Demo> = Arc::new(Counter(5));
    obj_arc.by_arc();

    let mut v6 = Counter(6);
    let obj_pin_mut: Pin<&mut dyn Demo> = Pin::new(&mut v6);
    obj_pin_mut.by_pin_mut();
}

trait Demo: Unpin {
    fn by_ref(&self);
    fn by_mut(&mut self);
    fn by_box(self: Box<Self>);
    fn by_rc(self: Rc<Self>);
    fn by_arc(self: Arc<Self>);
    fn by_pin_mut(self: Pin<&mut Self>);
}

struct Counter(i32);

impl Demo for Counter {
    fn by_ref(&self) {
        black_box(self.0);
    }

    fn by_mut(&mut self) {
        self.0 += 1;
        black_box(self.0);
    }

    fn by_box(self: Box<Self>) {
        black_box(self.0);
    }

    fn by_rc(self: Rc<Self>) {
        black_box(self.0);
    }

    fn by_arc(self: Arc<Self>) {
        black_box(self.0);
    }

    fn by_pin_mut(self: Pin<&mut Self>) {
        let this = self.get_mut();
        this.0 += 1;
        black_box(this.0);
    }
}
