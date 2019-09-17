use crate::heap::*;

pub unsafe extern "C" fn rt_lookup_property(
    heap: *mut Heap,
    obj: *mut u8,
    key: *mut u8,
    insert: bool,
) -> isize {
    let map = HObject::map_s(obj);
    let space = (*(map as *mut HMap)).space();
    let mask = HObject::mask(obj);

    let is_array = HValue::get_tag(obj) == HeapTag::Array;

    let key_ptr;
    let mut numkey = 0;
    let mut hash = 0;
    if is_array {
        numkey = HNumber::integral_value(rt_to_number(heap, key));
        key_ptr = HNumber::to_ptr(numkey);
        hash = crate::compute_hash(numkey as u64);
        if numkey < 0 {
            return HeapTag::Nil as u8 as isize;
        }

        if insert && HArray::length(obj, false) <= numkey as isize {
            HArray::set_length(obj, numkey as isize + 1);
        }
    } else {
        assert!(HValue::get_tag(obj) == HeapTag::Object);
        key_ptr = key;
    }
    if is_array && HArray::is_dense(obj) {
        let index = numkey * std::mem::size_of::<usize>() as i64;

        if index as u32 > mask {
            if insert {
                rt_grow_object(heap, obj, numkey as _);
                return rt_lookup_property(heap, obj, key_ptr, insert);
            } else {
                return HeapTag::Nil as u8 as isize;
            }
        }
        return HMap::SPACE_OFFSET as isize + (index as isize & mask as isize);
    } else {
        let start = hash & mask;
        let mut index = start;
        let mut key_slot;
        let mut needs_grow = true;
        loop {
            key_slot = *(space.offset(index as _) as *mut *mut u8);
            if key_slot == HNil::new() && (is_array && key_slot == key_ptr)
                || rt_strict_cmp(heap, key_slot, key) == 0
            {
                needs_grow = false;
                break;
            }

            index += std::mem::size_of::<usize>() as u32;
            index = index & mask;
            if index == start {
                break;
            }
        }
        if insert {
            if needs_grow {
                rt_grow_object(heap, obj, 0);
                return rt_lookup_property(heap, obj, key_ptr, insert);
            }
        }

        if key_slot == HNil::new() {
            let proto_slot = HObject::proto_slot_s(obj);
            *(proto_slot as *mut usize) = IC_DISABLED_VALUE;
        }

        return HMap::SPACE_OFFSET
            + index as isize
            + (mask as isize + std::mem::size_of::<isize>() as isize);
    }
}

pub unsafe extern "C" fn rt_grow_object(heap: *mut Heap, obj: *mut u8, min_size: usize) -> isize {
    let map_addr = HObject::map_slot_s(obj);
    let map = (*map_addr) as *mut HMap;
    let mut size = (*map).size() << 1;
    let mut original_size = size;
    if min_size > size as usize {
        size = min_size.pow(2) as u32;
    }

    let new_map = HMap::new_empty(&mut *heap, size as _);

    *map_addr = new_map;
    let mask = (size as usize)
        .wrapping_sub(1)
        .wrapping_mul(std::mem::size_of::<usize>());

    *HObject::mask_slot(obj) = mask as u32;

    if HValue::get_tag(obj) == HeapTag::Array && HArray::is_dense(obj) {
        original_size = original_size << 1;
        for i in 0..original_size {
            let value = *(*map).get_slot_address(i as _);
            if value == HNil::new() {
                continue;
            }
            *HObject::lookup_property(heap, obj, HNumber::to_ptr(i as _), true) = value;
        }
    } else {
        for i in 0..original_size {
            let key = *(*map).get_slot_address(i);
            if key == HNil::new() {
                continue;
            }
            let value = *(*map).get_slot_address(i + original_size);
            *HObject::lookup_property(heap, obj, key, true) = value;
        }
    }

    return 0;
}

pub unsafe extern "C" fn rt_to_number(heap: *mut Heap, value: *mut u8) -> *mut u8 {
    let tag = HValue::get_tag(value);
    match tag {
        HeapTag::String => {
            let string = HString::value(heap, value);
            let length = HString::static_length(value);
            let string = std::slice::from_raw_parts(string, length as _);
            let string = String::from_utf8(string.to_vec()).unwrap();
            return HNumber::newf(
                &mut *heap,
                Tenure::New,
                string.parse::<f64>().unwrap_or(std::f64::NAN),
            );
        }
        HeapTag::Boolean => {
            let val = HBoolean::value(value);
            return HNumber::newf(&mut *heap, Tenure::New, val as i32 as f64);
        }
        HeapTag::Number => return value,
        _ => return HNumber::newf(&mut *heap, Tenure::New, 0.0),
    }
}

pub unsafe extern "C" fn rt_strict_cmp(heap: *mut Heap, lhs: *mut u8, rhs: *mut u8) -> i32 {
    // Fast case - pointers are equal
    if lhs == rhs {
        return 0;
    }
    let tag = HValue::get_tag(lhs);
    let rtag = HValue::get_tag(rhs);
    // We can only compare objects with equal type
    if rtag != tag {
        return -1;
    }

    match tag {
        HeapTag::String => return rt_strcmp(heap, lhs, rhs),
        HeapTag::Boolean => {
            if HBoolean::value(lhs) == HBoolean::value(rhs) {
                return 0;
            } else {
                return -1;
            }
        }
        HeapTag::Number => {
            if HNumber::double_value(lhs) == HNumber::double_value(rhs) {
                return 0;
            } else {
                return -1;
            }
        }
        _ => return -1,
    }
}

pub unsafe extern "C" fn rt_strcmp(heap: *mut Heap, lhs: *mut u8, rhs: *mut u8) -> i32 {
    let lhs_len = HString::static_length(lhs);
    let rhs_len = HString::static_length(rhs);

    if lhs_len < rhs_len {
        return -1;
    } else if lhs_len > rhs_len {
        return 1;
    } else {
        return libc::strncmp(
            HString::value(heap, lhs) as *const _,
            HString::value(heap, rhs) as *const _,
            lhs_len as _,
        );
    };
}
