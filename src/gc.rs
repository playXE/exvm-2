use crate::heap::*;
use std::collections::VecDeque;
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct GCValue {
    pub value: *mut HValue,
    pub slot: *mut *mut u8,
}

impl GCValue {
    pub fn relocate(&mut self, address: *mut u8) {
        if !self.slot.is_null() {
            unsafe {
                *self.slot = address;
            }
        }
        unsafe {
            if (*self.value).is_gc_marked() {
                (*self.value).set_gc_mark(address);
            }
        }
    }
}

pub struct GC {
    grey: VecDeque<GCValue>,
    weak: VecDeque<GCValue>,
    black: VecDeque<GCValue>,
    heap: *mut Heap,
    tmp_space: Option<Space>,
    gc_type: GCType,
}

impl GC {
    pub fn new(heap: *mut Heap) -> GC {
        GC {
            heap,
            grey: VecDeque::new(),
            weak: VecDeque::new(),
            black: VecDeque::new(),
            tmp_space: None,
            gc_type: GCType::None,
        }
    }

    pub fn push_grey(&mut self, val: *mut HValue, reference: *mut *mut u8) {
        self.grey.push_back(GCValue {
            value: val,
            slot: reference,
        })
    }
    pub fn push_weak(&mut self, val: *mut HValue, reference: *mut *mut u8) {
        self.weak.push_back(GCValue {
            value: val,
            slot: reference,
        })
    }

    pub fn tmp_space(&mut self, s: Space) {
        self.tmp_space = Some(s);
    }

    pub fn collect_garbage(&mut self, stack_top: *mut u8) {
        unsafe {
            if (*self.heap).needs_gc == GCType::None {
                (*self.heap).needs_gc = GCType::NewSpace;
            }
            self.gc_type = (*self.heap).needs_gc;

            let space = if self.gc_type == GCType::NewSpace {
                (*self.heap).new_space
            } else {
                (*self.heap).old_space
            };
            self.tmp_space(Space::new((*space).page_size, self.heap));

            self.colour_persistent_handles();
            self.colour_frames(stack_top);

            while self.black.len() != 0 {
                let value = self.black.pop_front().unwrap();
                assert!((*value.value).is_soft_gc_marked());
                (*value.value).reset_soft_gc_mark();
            }
            self.relocate_weak_handles();
            self.handle_weak_refs();
            (*space).swap(self.tmp_space.as_mut().unwrap());
            if self.gc_type != GCType::NewSpace || (*self.heap).needs_gc == GCType::NewSpace {
                (*self.heap).needs_gc = GCType::None;
            } else {
                self.collect_garbage(stack_top);
            }
        }
    }

    fn relocate_weak_handles(&mut self) {
        unsafe {
            (*self.heap).references.retain(|_, val| {
                let ref_: &HValueRef = val;
                if ref_.is_weak() {
                    if HValue::is_unboxed(ref_.value as *mut _) {
                        return true;
                    }
                    let mut v;
                    if (*ref_.value).is_gc_marked() {
                        v = GCValue {
                            value: ref_.value,
                            slot: ref_.reference as *mut _,
                        };
                        v.relocate((*v.value).get_gc_mark());
                        v = GCValue {
                            value: ref_.value,
                            slot: ref_.value_ptr() as *mut _,
                        };
                        v.relocate((*v.value).get_gc_mark());
                        return true;
                    } else {
                        return false;
                    }
                } else {
                    true
                }
            });
        }
    }

    fn handle_weak_refs(&mut self) {
        unsafe {
            (*self.heap).weak_references.retain(|key, val| {
                let value: &mut HValueWeakRef = val;
                if !(*value.value).is_gc_marked() {
                    if self.is_in_current_space(value.value) {
                        if !value.callback.is_null() {
                            let callback: fn(*const HValue) = std::mem::transmute(value.callback);
                            callback(value.value);
                        }
                        return false;
                    } else {
                        value.value = HValue::cast((*value.value).get_gc_mark());
                        return true;
                    }
                }
                true
            })
        }
    }

    fn colour_frames(&mut self, stack_top: *mut u8) {
        unsafe {
            let mut frame = stack_top as *mut *mut u8;
            while frame != std::ptr::null_mut() {
                while !frame.is_null() && (*frame as usize) as u32 == ENTER_FRAME_TAG as u32 {
                    frame = (*frame.offset(1)) as *mut *mut u8;
                }
                if frame.is_null() {
                    break;
                }
                let value = *frame;
                if value.is_null() {
                    break;
                }
                //println!("{:p} {:p}", frame, value);
                if value != HNil::new() && !HValue::is_unboxed(value) && !value.is_null() {
                    println!("{:p} {:p}", value, frame);
                    self.push_grey(HValue::cast(value), frame);
                    self.process_grey();
                }
                frame = frame.add(1);
            }
        }
    }
    fn colour_persistent_handles(&mut self) {
        unsafe {
            for (_, value) in (*self.heap).references.iter() {
                let value: &HValueRef = value;
                if value.is_persistent() {
                    self.push_grey(value.value, value.reference as *mut _);
                    self.push_grey(value.value, value.value_ptr() as *mut _);
                }
            }
        }
    }

    fn process_grey(&mut self) {
        while let Some(item) = self.grey.pop_front() {
            let mut value = item;
            unsafe {
                if value.value == HValue::cast(HNil::new())
                    || HValue::is_unboxed((*value.value).addr())
                {
                    continue;
                }

                if !(*value.value).is_gc_marked() {
                    if !self.is_in_current_space(value.value) {
                        if !(*value.value).is_soft_gc_marked() {
                            (*value.value).set_soft_gc_mark();
                            self.black.push_back(value);
                            self.visit_value(value.value);
                        }
                        continue;
                    }

                    assert!(!(*value.value).is_soft_gc_marked());
                    let hvalue;
                    if self.gc_type == GCType::NewSpace {
                        hvalue = (*value.value).copy_to(
                            &mut *(*self.heap).old_space,
                            self.tmp_space.as_mut().unwrap(),
                        );
                    } else {
                        hvalue = (*value.value).copy_to(
                            self.tmp_space.as_mut().unwrap(),
                            &mut *(*self.heap).new_space,
                        );
                    }

                    value.relocate((*hvalue).addr());
                    self.visit_value(hvalue);
                } else {
                    value.relocate((*value.value).get_gc_mark());
                }
            }
        }
    }

    pub fn is_in_current_space(&self, value: *mut HValue) -> bool {
        unsafe {
            return (self.gc_type == GCType::OldSpace && (*value).generation() >= 5)
                || (self.gc_type == GCType::NewSpace && (*value).generation() < 5);
        }
    }

    fn visit_value(&mut self, value: *mut HValue) {
        unsafe {
            match (*value).tag() {
                HeapTag::Context => self.visit_ctx(value as *mut HContext),
                HeapTag::Function => self.visit_fn(value as *mut HFunction),
                HeapTag::Object => self.visit_obj(value as *mut HObject),
                HeapTag::Array => self.visit_array(value as *mut HArray),
                HeapTag::Map => self.visit_map(value as *mut HMap),
                HeapTag::String => {
                    let repr = HValue::get_repr((*value).addr());
                    match repr {
                        0x00 => return,
                        0x01 => self.visit_string(value),
                        _ => unreachable!(),
                    }
                }
                _ => return,
            }
        }
    }

    fn visit_ctx(&mut self, context: *mut HContext) {
        unsafe {
            if (*context).has_parent() {
                self.push_grey(HValue::cast((*context).parent()), (*context).parent_slot());
            }
            for i in 0..(*context).slots() {
                if !(*context).has_slot(i as _) {
                    continue;
                }
                self.push_grey(
                    (*context).get_slot(i as _),
                    (*context).get_slot_address(i as _),
                );
            }
        }
    }

    fn visit_fn(&mut self, fun: *mut HFunction) {
        unsafe {
            if !(*fun).parent_slot().is_null()
                && (*fun).parent() != BINDING_CONTEXT_TAG as u8 as *mut u8
            {
                self.push_grey(HValue::cast((*fun).parent()), (*fun).parent_slot());
            }
            if !(*fun).root_slot().is_null() {
                self.push_grey(HValue::cast((*fun).root()), (*fun).root_slot());
            }
        }
    }

    fn visit_obj(&mut self, obj: *mut HObject) {
        unsafe {
            if !(*obj).proto().is_null() {
                self.push_weak(HValue::cast((*obj).proto()), (*obj).proto_slot());
            }
            self.push_grey(HValue::cast((*obj).map()), (*obj).map_slot());
        }
    }

    fn visit_array(&mut self, arr: *mut HArray) {
        unsafe {
            self.push_grey(
                HValue::cast((*(arr as *mut HObject)).map()),
                (*(arr as *mut HObject)).map_slot(),
            )
        }
    }

    fn visit_map(&mut self, map: *mut HMap) {
        unsafe {
            let size = (*map).size() << 1;
            for i in 0..size {
                if (*map).is_empty_slot(i) {
                    continue;
                }
                self.push_grey((*map).get_slot(i), (*map).get_slot_address(i));
            }
        }
    }

    fn visit_string(&mut self, value: *mut HValue) {
        unsafe {
            self.push_grey(
                HValue::cast(HString::left_cons((*value).addr())),
                HString::left_cons_slot((*value).addr()),
            );
            self.push_grey(
                HValue::cast(HString::right_cons((*value).addr())),
                HString::right_cons_slot((*value).addr()),
            );
        }
    }
}
