extern "C" {
    fn malloc(x: usize) -> *mut u8;
    fn free(x: *mut u8);
    fn memset(ptr: *mut u8, val: i32, size: usize);
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub struct Page {
    pub(super) data: *mut u8,
    pub(super) top: *mut u8,
    pub(super) limit: *mut u8,
    pub(super) size: usize,
}

pub const PAGE_SIZE: usize = 4096;

impl Page {
    #[inline]
    pub fn new(x: usize) -> Page {
        let data = unsafe { malloc(x) };
        unsafe {
            for i in 0..x {
                *data.offset(i as _) = 0;
            }
        }
        Page {
            size: x,
            data,
            top: unsafe { data.offset(1) },
            limit: unsafe { data.offset(x as isize) },
        }
    }
}

use std::collections::HashMap;

pub type HValueRefMap = HashMap<usize, HValueRef>;
pub type HValueRefList = Vec<HValueRef>;
pub type HValueWeakRefMap = HashMap<usize, HValueWeakRef>;

pub struct Heap {
    pub new_space: *mut Space,
    pub old_space: *mut Space,
    pub last_stack: *mut u8,
    pub last_frame: *mut u8,
    pub pending_exception: *mut u8,
    pub needs_gc: GCType,
    pub factory: *mut HValue,
    pub references: HValueRefMap,
    pub weak_references: HValueWeakRefMap,
}

use std::alloc::Layout;
/*
impl Drop for Heap {
    fn drop(&mut self) {
        unsafe {
            std::ptr::drop_in_place(self.new_space);
            std::ptr::drop_in_place(self.old_space);
            std::alloc::dealloc(self.new_space as _, Layout::new::<Space>());
            std::alloc::dealloc(self.old_space as _, Layout::new::<Space>());
        }
    }
}*/

pub struct Ptr<T: ?Sized> {
    pub val: *mut T,
}

impl<T> Ptr<T> {
    pub fn from_raw(p: *mut T) -> Self {
        Self { val: p }
    }
}

impl<T: Drop> Ptr<T> {
    pub fn drop(&self) {
        unsafe { std::ptr::drop_in_place(self.val) };
    }
}

impl<T: ?Sized> std::ops::Deref for Ptr<T> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe { &*self.val }
    }
}

impl<T: ?Sized> std::ops::DerefMut for Ptr<T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.val }
    }
}

impl Heap {
    pub fn new(page_size: usize) -> Ptr<Heap> {
        unsafe {
            let h = Heap {
                needs_gc: GCType::None,
                old_space: std::ptr::null_mut(),
                new_space: std::ptr::null_mut(),
                last_stack: std::ptr::null_mut(),
                last_frame: std::ptr::null_mut(),
                pending_exception: std::ptr::null_mut(),
                references: HashMap::new(),
                weak_references: HashMap::new(),
                factory: std::ptr::null_mut(),
            };

            let heap_ptr = Box::into_raw(Box::new(h));
            let heap: &mut Heap = &mut *heap_ptr;
            heap.old_space = Box::into_raw(Box::new(Space::new(page_size, heap_ptr)));
            heap.new_space = Box::into_raw(Box::new(Space::new(page_size, heap_ptr)));
            heap.factory = HValue::cast(HObject::new_empty(heap, 128));
            let mut f = heap.factory;
            heap.reference(RefType::Persistent, &mut f, f);
            Ptr::from_raw(heap_ptr)
        }
    }

    pub fn reference(&mut self, ty: RefType, reference: *mut *mut HValue, value: *mut HValue) {
        let ref_ = HValueRef {
            ty,
            reference,
            value,
        };
        self.references.insert(reference as _, ref_);
    }

    pub fn allocate_tagged(&mut self, tag: HeapTag, tenure: Tenure, bytes: usize) -> *mut u8 {
        let result = unsafe { self.space(tenure).allocate(bytes + 8) };
        let mut qtag = tag as u8 as isize;
        if tenure == Tenure::Old {
            let bit_offset = (HValue::GENERATION_OFF - interior_offset(0)) << 3;
            qtag = qtag | (5isize.wrapping_shl(bit_offset as _));
        }
        unsafe {
            *(result.offset(HValue::TAG_OFFSET) as *mut isize) = qtag;
        }
        result
    }
}

pub static mut HEAP: *mut Heap = std::ptr::null_mut();

pub fn get_heap() -> &'static mut Heap {
    unsafe {
        assert!(!HEAP.is_null());
        &mut *HEAP
    }
}

impl Heap {
    pub unsafe fn space(&mut self, t: Tenure) -> &mut Space {
        if t == Tenure::Old {
            return &mut *self.old_space;
        } else {
            return &mut *self.new_space;
        }
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct Space {
    pub top: *mut *mut u8,
    pub limit: *mut *mut u8,
    pub pages: Vec<Page>,
    pub page_size: usize,
    pub size: usize,
    pub size_limit: usize,
    pub heap: *mut Heap,
}

impl Drop for Space {
    fn drop(&mut self) {
        self.pages.clear();
    }
}

impl Space {
    pub fn new(page_size: usize, heap: *mut Heap) -> Space {
        let mut space = Space {
            page_size,
            size: 0,
            pages: vec![],
            top: std::ptr::null_mut(),
            limit: std::ptr::null_mut(),
            size_limit: 0,
            heap,
        };

        let page = Page::new(page_size);

        space.select(&page);
        space.pages.push(page);

        space
    }

    pub fn compute_size_limit(&mut self) {
        self.size_limit = self.size << 1;
    }

    pub fn select(&mut self, page: &Page) {
        self.top = (&page.top) as *const *mut u8 as *mut *mut _;
        self.limit = (&page.limit) as *const *mut u8 as *mut *mut _;
    }

    pub fn allocate(&mut self, bytes: usize) -> *mut u8 {
        assert!(bytes != 0);
        let even_bytes = bytes + (bytes & 0x01);

        unsafe {
            let place_in_current = (*self.top).offset(even_bytes as _) <= *self.limit;
            if !place_in_current {
                let mut i = 0;
                let mut gap_found = false;
                for item in self.pages.clone().iter() {
                    if (*self.top).offset(even_bytes as _) > *self.limit {
                        if i < self.pages.len() {
                            gap_found = true;
                        } else {
                            gap_found = false;
                        }
                        i = i + 1;
                        self.select(&item);
                    } else {
                        break;
                    }
                }

                if !gap_found {
                    if self.size > self.size_limit {
                        let heap: &mut Heap = &mut *self.heap;
                        if self as *const Space as *const u8 == heap.new_space as *const u8 {
                            heap.needs_gc = GCType::NewSpace;
                        } else {
                            heap.needs_gc = GCType::OldSpace;
                        }
                    }
                    self.add_page(even_bytes + 1);
                }
            }
            let result = *self.top;
            (*self.top) = (*self.top).offset(even_bytes as _);
            return result;
        }
    }

    pub fn clear(&mut self) {
        self.size = 0;
        for page in self.pages.iter() {
            unsafe {
                free(page.data);
            }
        }
        self.pages.clear();
    }
    pub fn swap(&mut self, space: &mut Space) {
        self.clear();
        while space.pages.len() != 0 {
            self.pages.push(space.pages.pop().unwrap());
            self.size += self.pages.last().unwrap().size;
        }

        self.select(&self.pages.first().unwrap().clone());
        self.compute_size_limit();
    }

    pub fn add_page(&mut self, size: usize) {
        #[inline(always)]
        fn roundup(value: u32, to: u32) -> u32 {
            if value % to == 0 {
                return value;
            }
            return value + to;
        }
        let real_size = roundup(size as _, self.page_size as _) as usize;

        let page = Page::new(real_size);
        self.size += real_size;
        self.select(&page);
        self.pages.push(page);
    }
}

#[derive(Copy, Clone, Debug, Hash, PartialEq, PartialOrd, Ord, Eq)]
#[repr(u8)]
pub enum HeapTag {
    Nil = 0x01,
    Context,
    Boolean,
    Number,
    String,
    Object,
    Array,
    Function,
    ExternData,
    Map,
}
#[derive(Copy, Clone, Debug, Hash, PartialEq, PartialOrd, Ord, Eq)]
#[repr(u8)]
pub enum Tenure {
    New = 0,
    Old = 1,
}

#[derive(Copy, Clone, Debug, Hash, PartialEq, PartialOrd, Ord, Eq)]
#[repr(u8)]
pub enum GCType {
    None = 0,
    NewSpace = 1,
    OldSpace = 2,
}

#[derive(Copy, Clone, Debug, Hash, PartialEq, PartialOrd, Ord, Eq)]
#[repr(u8)]
pub enum RefType {
    Weak,
    Persistent,
}

pub const MIN_OLD_SPACE_GEN: u8 = 5;
pub const MIN_FACTORY_SIZE: u8 = 128;
pub const ENTER_FRAME_TAG: usize = 0xFEEDBEEE;
pub const BINDING_CONTEXT_TAG: usize = 0x0DEC0DEC;
pub const IC_DISABLED_VALUE: usize = 0xABBAABBA;
pub const IC_ZAP_VALUE: usize = 0xABBADEEC;

pub trait HValTrait: Sized + Copy {
    fn addr(&self) -> *mut u8 {
        unsafe { std::mem::transmute(self) }
    }

    const TAG: HeapTag;
}

const fn interior_offset(x: isize) -> isize {
    return x * std::mem::size_of::<isize>() as isize - 1;
}

#[derive(Copy, Clone, Debug, Hash, PartialEq, PartialOrd, Ord, Eq)]
pub struct HValue;

impl HValTrait for HValue {
    const TAG: HeapTag = HeapTag::Nil;
}

impl HValue {
    pub const TAG_OFFSET: isize = interior_offset(0);
    pub const GC_MARK_OFF: isize = interior_offset(1) - 1;
    pub const GC_FORWARD_OFF: isize = interior_offset(1);
    pub const REPR_OFF: isize = interior_offset(0) + 1;
    pub const GENERATION_OFF: isize = interior_offset(0) + 2;

    pub fn is_soft_gc_marked(&self) -> bool {
        if Self::is_unboxed(self.addr()) {
            return false;
        }
        unsafe {
            return (*self.addr().offset(HValue::GC_MARK_OFF)) & 0x40 != 0;
        }
    }

    pub fn set_soft_gc_mark(&self) {
        unsafe {
            *(self.addr().offset(Self::GC_MARK_OFF)) |= 0x40;
            //*(self.addr().offset(Self::GC_FORWARD_OFF) as *mut *mut u8) = new_addr;
        }
    }

    pub fn reset_soft_gc_mark(&self) {
        unsafe {
            if self.is_soft_gc_marked() {
                *(self.addr().offset(Self::GC_MARK_OFF)) ^= 0x40;
            }
        }
    }

    pub fn generation(&self) -> u8 {
        return unsafe { *self.addr().offset(Self::GENERATION_OFF) };
    }
    pub fn increment_generation(&self) {
        if self.generation() < 5 {
            unsafe {
                let slot = self.addr().offset(Self::GENERATION_OFF);
                *slot = *slot + 1;
            }
        }
    }

    #[inline]
    pub fn is_gc_marked(&self) -> bool {
        if Self::is_unboxed(self.addr()) {
            return false;
        }
        unsafe {
            return (*self.addr().offset(HValue::GC_MARK_OFF)) & 0x80 != 0;
        }
    }
    #[inline]
    pub const fn is_unboxed(addr: *mut u8) -> bool {
        return unsafe { (addr as usize & 0x01) == 0 };
    }
    #[inline]
    pub const fn cast(addr: *mut u8) -> *mut HValue {
        return addr as *mut HValue;
    }

    pub fn tag(&self) -> HeapTag {
        Self::get_tag(self.addr())
    }
    pub fn as_<T: HValTrait>(&self) -> *mut T {
        assert!(self.tag() == T::TAG);
        return unsafe { std::mem::transmute(self) };
    }

    pub fn get_tag(addr: *mut u8) -> HeapTag {
        if addr == (HeapTag::Nil as u8 as *mut u8) {
            return HeapTag::Nil;
        }

        if Self::is_unboxed(addr) {
            return HeapTag::Number;
        }

        return unsafe { std::mem::transmute(*addr.offset(Self::TAG_OFFSET)) };
    }

    pub fn get_repr(addr: *mut u8) -> u8 {
        return unsafe { std::mem::transmute(*(addr.offset(Self::REPR_OFF))) };
    }

    pub fn get_gc_mark(&self) -> *mut u8 {
        return unsafe { *(self.addr().offset(Self::GC_FORWARD_OFF) as *mut *mut u8) };
    }

    pub fn is_marked(&self) -> bool {
        if HValue::is_unboxed(self.addr()) {
            return false;
        }

        return unsafe { (*self.addr().offset(Self::GC_MARK_OFF) & 0x80) != 0 };
    }

    pub fn set_gc_mark(&self, new_addr: *mut u8) {
        unsafe {
            *(self.addr().offset(Self::GC_MARK_OFF)) |= 0x80;
            *(self.addr().offset(Self::GC_FORWARD_OFF) as *mut *mut u8) = new_addr;
        }
    }

    pub fn size(&self) -> usize {
        const PTR_SIZE: usize = 8;
        unsafe {
            let mut size = PTR_SIZE;
            match self.tag() {
                HeapTag::Context => {
                    size += (2 * (*self.as_::<HContext>()).slots() as usize) * PTR_SIZE;
                }
                HeapTag::Function => {
                    size += 4 * PTR_SIZE;
                }
                HeapTag::Number => {
                    size += 8;
                }

                HeapTag::Boolean => {
                    size += 8;
                }

                HeapTag::String => {
                    size += 2 * PTR_SIZE;
                    match Self::get_repr(self.addr()) {
                        0 => {
                            size += (*self.as_::<HString>()).length() as usize;
                        }
                        _ => {
                            size += 2 * PTR_SIZE;
                        }
                    }
                }
                HeapTag::Object => {
                    size += 3 * PTR_SIZE;
                }
                HeapTag::Array => {
                    size += 4 * PTR_SIZE;
                }
                HeapTag::Map => {
                    size += (1 + ((*self.as_::<HMap>()).size() as usize) << 1) * PTR_SIZE;
                }

                _ => (),
            }

            size
        }
    }
    pub fn copy_to(&self, old_space: &mut Space, new_space: &mut Space) -> *mut HValue {
        assert!(!Self::is_unboxed(self.addr()));
        const PTR_SIZE: usize = std::mem::size_of::<usize>();
        unsafe {
            let mut size = PTR_SIZE;
            match self.tag() {
                HeapTag::Context => {
                    size += (2 * (*self.as_::<HContext>()).slots() as usize) * PTR_SIZE;
                }
                HeapTag::Function => {
                    size += 4 * PTR_SIZE;
                }
                HeapTag::Number => {
                    size += 8;
                }

                HeapTag::Boolean => {
                    size += 8;
                }

                HeapTag::String => {
                    size += 2 * PTR_SIZE;
                    match Self::get_repr(self.addr()) {
                        0 => {
                            size += (*self.as_::<HString>()).length() as usize;
                        }
                        _ => {
                            size += 2 * PTR_SIZE;
                        }
                    }
                }
                HeapTag::Object => {
                    size += 3 * PTR_SIZE;
                }
                HeapTag::Array => {
                    size += 4 * PTR_SIZE;
                }
                HeapTag::Map => {
                    size += (1 + ((*self.as_::<HMap>()).size() as usize) << 1) * PTR_SIZE;
                }

                _ => unimplemented!(),
            }

            self.increment_generation();
            let result;
            if self.generation() >= 5 {
                result = old_space.allocate(size);
            } else {
                result = new_space.allocate(size);
            }
            std::ptr::copy_nonoverlapping(
                self.addr().offset(interior_offset(0)),
                result.offset(interior_offset(0)),
                size,
            );

            return HValue::cast(result);
        }
    }

    /*pub fn copy_to(&self, addr: &mut crate::gc::Address) -> (*mut u8, usize) {
        const PTR_SIZE: usize = 8;
        unsafe {
            let mut size = PTR_SIZE;
            match self.tag() {
                HeapTag::Context => {
                    size += (2 * (*self.as_::<HContext>()).slots() as usize) * PTR_SIZE;
                }
                HeapTag::Function => {
                    size += 4 * PTR_SIZE;
                }
                HeapTag::Number => {
                    size += 8;
                }

                HeapTag::Boolean => {
                    size += 8;
                }

                HeapTag::String => {
                    size += 2 * PTR_SIZE;
                    match Self::get_repr(self.addr()) {
                        0 => {
                            size += (*self.as_::<HString>()).length() as usize;
                        }
                        _ => {
                            size += 2 * PTR_SIZE;
                        }
                    }
                }
                HeapTag::Object => {
                    size += 3 * PTR_SIZE;
                }
                HeapTag::Array => {
                    size += 4 * PTR_SIZE;
                }
                HeapTag::Map => {
                    size += (1 + ((*self.as_::<HMap>()).size() as usize) << 1) * PTR_SIZE;
                }

                _ => unimplemented!(),
            }
            let result = self.addr().offset(interior_offset(0));
            std::ptr::copy_nonoverlapping(
                result,
                addr.to_mut_ptr::<u8>().offset(interior_offset(0)),
                size,
            );

            return (result, size);
        }
    }*/
}
#[derive(Copy, Clone, Debug, Hash, PartialEq, PartialOrd, Ord, Eq)]
pub struct HContext;

impl HValTrait for HContext {
    const TAG: HeapTag = HeapTag::Context;
}

impl HContext {
    pub fn parent_slot(&self) -> *mut *mut u8 {
        return unsafe { (self.addr().offset(Self::PARENT_OFFSET)) as *mut *mut _ };
    }

    pub fn parent(&self) -> *mut u8 {
        unsafe { *self.parent_slot() }
    }

    pub fn has_parent(&self) -> bool {
        !self.parent().is_null()
    }

    pub fn get_slot(&self, idx: u32) -> *mut HValue {
        unsafe { HValue::cast(*self.get_slot_address(idx)) }
    }

    pub fn has_slot(&self, idx: u32) -> bool {
        return unsafe { *self.get_slot_address(idx) != HeapTag::Nil as u8 as *mut u8 };
    }

    pub fn get_slot_address(&self, idx: u32) -> *mut *mut u8 {
        return unsafe { (self.addr().offset(Self::get_index_disp(idx))) as *mut *mut u8 };
    }

    pub fn get_index_disp(index: u32) -> isize {
        return interior_offset(3 + index as isize);
    }
    pub fn slots(&self) -> usize {
        return unsafe { *(self.addr().offset(Self::SLOTS_OFFSET) as *mut usize) };
    }
    pub const PARENT_OFFSET: isize = interior_offset(1);
    pub const SLOTS_OFFSET: isize = interior_offset(2);
}

#[derive(Copy, Clone, Debug, Hash, PartialEq, PartialOrd, Ord, Eq)]
#[repr(u8)]
pub enum StrRepr {
    Normal = 0x00,
    Cons = 0x01,
}

#[derive(Copy, Clone, Debug, Hash, PartialEq, PartialOrd, Ord, Eq)]
pub struct HString;

impl HValTrait for HString {
    const TAG: HeapTag = HeapTag::String;
}

impl HString {
    pub const HASH_OFFSET: isize = interior_offset(1);
    pub const LENGTH_OFFSET: isize = interior_offset(2);
    pub const VALUE_OFFSET: isize = interior_offset(3);
    pub const LEFT_CONS_OFFSET: isize = interior_offset(3);
    pub const RIGHT_CONS_OFFSET: isize = interior_offset(4);
    pub const MIN_CONS_LEN: usize = 24;

    pub fn static_length(addr: *mut u8) -> u32 {
        return unsafe { *(addr.offset(HString::LENGTH_OFFSET) as *mut u32) };
    }

    pub fn left_cons(addr: *mut u8) -> *mut u8 {
        unsafe { *HString::left_cons_slot(addr) }
    }
    pub fn right_cons(addr: *mut u8) -> *mut u8 {
        unsafe { *HString::right_cons_slot(addr) }
    }

    pub fn left_cons_slot(addr: *mut u8) -> *mut *mut u8 {
        unsafe { addr.offset(Self::LEFT_CONS_OFFSET) as *mut *mut u8 }
    }
    pub fn right_cons_slot(addr: *mut u8) -> *mut *mut u8 {
        unsafe { addr.offset(Self::RIGHT_CONS_OFFSET) as *mut *mut u8 }
    }

    pub fn length(&self) -> u32 {
        Self::static_length(self.addr())
    }
}
#[derive(Copy, Clone, Debug, Hash, PartialEq, PartialOrd, Ord, Eq)]
pub struct HMap;

impl HValTrait for HMap {
    const TAG: HeapTag = HeapTag::Map;
}

impl HMap {
    pub fn new_empty(heap: &mut Heap, mut size: usize) -> *mut u8 {
        unsafe {
            let map = heap.allocate_tagged(
                HeapTag::Map,
                Tenure::New,
                ((size << 1) + 1) * std::mem::size_of::<usize>(),
            );
            size = (size << 1) * std::mem::size_of::<usize>();
            memset(map.offset(Self::SPACE_OFFSET), 0x00, size);
            for i in 0..size {
                *map.offset(i as isize + Self::SPACE_OFFSET as isize) = HeapTag::Nil as u8;
            }

            map
        }
    }

    pub fn size(&self) -> u32 {
        return unsafe { *(self.addr().offset(Self::SIZE_OFFSET) as *mut u32) };
    }

    pub fn get_slot_address(&self, index: u32) -> *mut *mut u8 {
        return unsafe {
            self.space()
                .offset(index as isize * std::mem::size_of::<usize>() as isize)
                as *mut *mut _
        };
    }
    pub fn is_empty_slot(&self, index: u32) -> bool {
        unsafe { *Self.get_slot_address(index) == HNil::new() }
    }
    pub fn get_slot(&self, index: u32) -> *mut HValue {
        return unsafe { HValue::cast(*self.get_slot_address(index)) };
    }

    pub fn has_slot(&self, index: u32) -> bool {
        unsafe { *self.get_slot_address(index) != HeapTag::Nil as u8 as *mut u8 }
    }

    pub fn space(&self) -> *mut u8 {
        unsafe { self.addr().offset(Self::SPACE_OFFSET) }
    }
    pub const SPACE_OFFSET: isize = interior_offset(2);
    pub const SIZE_OFFSET: isize = interior_offset(1);
}
#[derive(Copy, Clone, Debug, Hash, PartialEq, PartialOrd, Ord, Eq)]
pub struct HArray;

impl HValTrait for HArray {
    const TAG: HeapTag = HeapTag::Array;
}
#[derive(Copy, Clone, Debug, Hash, PartialEq, PartialOrd, Ord, Eq)]
pub struct HNumber;

impl HNumber {
    pub fn new(value: i64) -> *mut u8 {
        return Self::tag(value) as *mut _;
    }
    pub fn newf(heap: &mut Heap, tenure: Tenure, value: f64) -> *mut u8 {
        unsafe {
            let result = heap.allocate_tagged(HeapTag::Number, tenure, 8);
            *(result.offset(Self::VALUE_OFFSET) as *mut f64) = value;
            return result;
        }
    }

    pub const VALUE_OFFSET: isize = interior_offset(1);
    pub const fn tag(value: i64) -> i64 {
        return value << 1;
    }

    pub const fn untag(value: i64) -> i64 {
        return value >> 1;
    }

    pub fn to_ptr(value: i64) -> *mut u8 {
        let oval = Self::tag(value) as *mut u8;
        oval
    }
    pub fn integral_value(addr: *mut u8) -> i64 {
        if HValue::is_unboxed(addr) {
            return Self::untag(addr as _);
        } else {
            return unsafe { *(addr.offset(Self::VALUE_OFFSET) as *const f64) as i64 };
        }
    }

    pub fn double_value(addr: *mut u8) -> f64 {
        if HValue::is_unboxed(addr) {
            return f64::from_bits(Self::untag(addr as _) as _);
        } else {
            return unsafe { *(addr.offset(Self::VALUE_OFFSET) as *const f64) };
        }
    }
}

impl HArray {
    pub fn length(obj: *mut u8, shrink: bool) -> usize {
        unsafe {
            let mut result = *(obj.offset(Self::LENGTH_OFFSET) as *mut usize);
            if shrink {
                let mut shrinked = result;
                let mut shrinked_ptr: *mut u8;
                let mut slot: *mut *mut u8;
                loop {
                    if shrinked < 0 {
                        break;
                    } else {
                        shrinked -= 1;
                        shrinked_ptr = HNumber::tag(shrinked as i64) as *mut u8;
                        slot = std::ptr::null_mut();
                    }
                    if *slot != HNil::new() {
                        break;
                    }
                }

                if result != (shrinked + 1) {
                    result = shrinked + 1;
                    HArray::set_length(obj, result);
                }
            }

            result
        }
    }

    pub fn set_length(obj: *mut u8, len: usize) {
        unsafe {
            *(obj.offset(Self::LENGTH_OFFSET) as *mut usize) = len;
        }
    }

    pub const VAR_ARG_LEN: usize = 16;
    pub const DENSE_LENGTH_MAX: usize = 128;
    pub const LENGTH_OFFSET: isize = interior_offset(4);
}
#[derive(Copy, Clone, Debug, Hash, PartialEq, PartialOrd, Ord, Eq)]
pub struct HObject;

impl HValTrait for HObject {
    const TAG: HeapTag = HeapTag::Object;
}

impl HObject {
    pub fn new_empty(heap: &mut Heap, size: usize) -> *mut u8 {
        unsafe {
            let obj = heap.allocate_tagged(
                HeapTag::Object,
                Tenure::New,
                3 * std::mem::size_of::<usize>(),
            );
            HObject::init(heap, obj, size);

            obj
        }
    }
    pub fn init(heap: &mut Heap, obj: *mut u8, size: usize) {
        unsafe {
            *(obj.offset(Self::MASK_OFFSET) as *mut isize) =
                (size as isize - 1) * std::mem::size_of::<usize>() as isize;
            let map = HMap::new_empty(heap, size);
            *(obj.offset(Self::MAP_OFFSET) as *mut *mut u8) = map;
            *(obj.offset(Self::PROTO_OFFSET) as *mut *mut u8) = map;
        }
    }

    pub fn map_slot_s(addr: *mut u8) -> *mut *mut u8 {
        return unsafe { addr.offset(Self::MAP_OFFSET) as *mut *mut _ };
    }

    pub fn map_s(addr: *mut u8) -> *mut u8 {
        return unsafe { *Self::map_slot_s(addr) };
    }

    pub fn map(&self) -> *mut u8 {
        Self::map_s(self.addr())
    }

    pub fn map_slot(&self) -> *mut *mut u8 {
        Self::map_slot_s(self.addr())
    }

    pub fn proto_slot_s(addr: *mut u8) -> *mut *mut u8 {
        return unsafe { addr.offset(Self::PROTO_OFFSET) as *mut *mut _ };
    }

    pub fn proto_s(addr: *mut u8) -> *mut u8 {
        unsafe { *Self::proto_slot_s(addr) }
    }

    pub fn proto(&self) -> *mut u8 {
        Self::proto_s(self.addr())
    }

    pub fn proto_slot(&self) -> *mut *mut u8 {
        Self::proto_slot_s(self.addr())
    }

    pub const MASK_OFFSET: isize = interior_offset(1);
    pub const MAP_OFFSET: isize = interior_offset(2);
    pub const PROTO_OFFSET: isize = interior_offset(3);
}
#[derive(Copy, Clone, Debug, Hash, PartialEq, PartialOrd, Ord, Eq)]
pub struct HFunction;

impl HValTrait for HFunction {
    const TAG: HeapTag = HeapTag::Function;
}

impl HFunction {
    pub const PARENT_OFFSET: isize = interior_offset(1);
    pub const CODE_OFFSET: isize = interior_offset(2);
    pub const ROOT_OFFSET: isize = interior_offset(3);
    pub const ARGC_OFFSET: isize = interior_offset(4);

    pub fn root_s(addr: *mut u8) -> *mut u8 {
        unsafe { *(addr.offset(Self::ROOT_OFFSET) as *mut *mut u8) }
    }

    pub fn root_slot(&self) -> *mut *mut u8 {
        unsafe { self.addr().offset(Self::ROOT_OFFSET) as *mut *mut u8 }
    }

    pub fn root(&self) -> *mut u8 {
        unsafe { *(self.root_slot()) }
    }

    pub fn argc(&self) -> u32 {
        unsafe { *self.argc_off() }
    }

    pub fn parent(&self) -> *mut u8 {
        unsafe { *self.parent_slot() }
    }

    pub fn parent_slot(&self) -> *mut *mut u8 {
        unsafe { self.addr().offset(Self::PARENT_OFFSET) as *mut *mut _ }
    }

    pub fn argc_off(&self) -> *mut u32 {
        unsafe { self.addr().offset(Self::ARGC_OFFSET) as *mut u32 }
    }
}

#[derive(Clone, PartialEq, PartialOrd, Copy, Debug, Hash)]
pub struct HValueRef {
    pub ty: RefType,
    pub reference: *mut *mut HValue,
    pub value: *mut HValue,
}

impl HValueRef {
    pub fn value_ptr(&self) -> *const *mut HValue {
        return &self.value as *const _;
    }
    pub fn is_weak(&self) -> bool {
        self.ty == RefType::Weak
    }
    pub fn is_persistent(&self) -> bool {
        self.ty == RefType::Persistent
    }
    pub fn make_weak(&mut self) {
        self.ty = RefType::Weak;
    }
    pub fn make_persistent(&mut self) {
        self.ty = RefType::Persistent;
    }
}

#[derive(Clone, PartialEq, PartialOrd, Copy, Debug, Hash)]
pub struct HValueWeakRef {
    pub value: *mut HValue,
    pub callback: *const u8,
}

#[derive(Copy, Clone, Debug, Hash, PartialEq, PartialOrd, Ord, Eq)]
pub struct HNil;

impl HValTrait for HNil {
    const TAG: HeapTag = HeapTag::Nil;
}

impl HNil {
    pub fn new() -> *mut u8 {
        return Self::TAG as u8 as *mut u8;
    }
}
