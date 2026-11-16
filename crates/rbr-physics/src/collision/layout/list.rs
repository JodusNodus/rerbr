//! E37 intrusive lists: the head and node accessors, the clears, the insert, unlink, release and drains.

use super::event_pool::{FREE_LIST_COUNT, FREE_LIST_HEAD};
use crate::mem::Mem;

const FREE_LIST_TAIL: u32 = 0x008e_f7f8;

/// E37First (0x4f4b50): return the first word at the list head.
pub fn first(mem: &Mem, object: u32) -> u32 {
    mem.u32(object)
}

/// E37At8 (0x4f4b70) (name ours): the list's count (its head's word at +8).
pub fn count(mem: &Mem, object: u32) -> u32 {
    mem.u32(object + 8)
}

/// E37ListOps::Insert (0x4f4b90): push a node at the list head and update its links/count.
pub fn insert(mem: &mut Mem, head: u32, node: u32) {
    let first = mem.u32(head);
    mem.set_u32(node + 0x0c, first);
    mem.set_u32(node + 0x10, 0);
    if first != 0 {
        mem.set_u32(first + 0x10, node);
        mem.set_u32(head, node);
    } else {
        mem.set_u32(head + 4, node);
        mem.set_u32(head, node);
    }
    mem.set_u32(node + 0x14, head);
    mem.set_u32(head + 8, mem.u32(head + 8).wrapping_add(1));
}

/// E37Unlink (0x4f4bc0): unlink the three-word node and clear its local links.
pub fn unlink(mem: &mut Mem, node: u32) {
    let next = mem.u32(node);
    let prev = mem.u32(node + 4);
    let head = mem.u32(node + 8);
    if head != 0 {
        if next != 0 {
            mem.set_u32(next + 0x10, prev);
            if prev != 0 {
                mem.set_u32(prev + 0x0c, next);
            } else {
                mem.set_u32(head, next);
                mem.set_u32(node, 0);
                mem.set_u32(node + 4, 0);
                mem.set_u32(node + 8, 0);
                return;
            }
        } else if prev != 0 {
            mem.set_u32(prev + 0x0c, 0);
            mem.set_u32(head + 4, prev);
        } else {
            mem.set_u32(head, 0);
            mem.set_u32(head + 4, 0);
        }
    }
    mem.set_u32(node, 0);
    mem.set_u32(node + 4, 0);
    mem.set_u32(node + 8, 0);
}

/// E37PushFront (0x4f4c30): prepend a node to the global collision-event free list.
pub fn push_front(mem: &mut Mem, node: u32) {
    let old_head = mem.u32(FREE_LIST_HEAD);
    mem.set_u32(node + 0x0c, old_head);
    mem.set_u32(node + 0x10, 0);
    if old_head == 0 {
        mem.set_u32(FREE_LIST_TAIL, node);
    } else {
        mem.set_u32(old_head + 0x10, node);
    }
    mem.set_u32(FREE_LIST_HEAD, node);
    mem.set_u32(node + 0x14, FREE_LIST_HEAD);
    mem.set_u32(FREE_LIST_COUNT, mem.u32(FREE_LIST_COUNT).wrapping_add(1));
}

/// E37ListOps::Release (0x4f4ea0): decrement and unlink only when the link belongs to this head.
pub fn release(mem: &mut Mem, head: u32, node: u32) {
    if mem.u32(node + 8) == head {
        mem.set_u32(head + 8, mem.u32(head + 8).wrapping_sub(1));
        unlink(mem, node);
    }
}

/// E37ListOps::Drain (0x4f4ec0): unlink owned events, optionally demote and pool each one.
pub fn drain(mem: &mut Mem, head: u32, flag: u32) {
    let mut left = mem.u32(head + 8);
    while left != 0 {
        let node = mem.u32(head);
        let link = if node == 0 { 0 } else { node + 0x0c };
        if mem.u32(link + 8) == head {
            mem.set_u32(head + 8, mem.u32(head + 8).wrapping_sub(1));
            unlink(mem, link);
        }
        if flag != 0 && node != 0 {
            super::event_pool::release_event(mem, node, 1);
        }
        left = mem.u32(head + 8);
    }
}
