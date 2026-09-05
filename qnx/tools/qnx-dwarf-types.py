#!/usr/bin/env python3
"""Print exact QNX type layouts from the DWARF in a QNX ``libc.so.5.sym``.

The QNX 7.1 SDP ships a debug build of the C library alongside the stripped
runtime one. That debug build carries full ``.debug_info``, which is the
authoritative source for every struct layout, enumeration and message type the
relibc QNX backend has to match. This script is how the constants and
``#[repr(C)]`` definitions under ``src/platform/qnx7/abi`` were derived, and it
is kept in the tree so they can be re-derived or extended.

Usage::

    python3 qnx/tools/qnx-dwarf-types.py libc.so.5.sym struct:stat struct:_io_connect
    python3 qnx/tools/qnx-dwarf-types.py libc.so.5.sym enum:_io_msg_types
    python3 qnx/tools/qnx-dwarf-types.py libc.so.5.sym --grep '_io_.*'

It needs ``pyelftools``; no QNX SDP is required.
"""

import re
import sys

from elftools.elf.elffile import ELFFile


def die_name(die):
    value = die.attributes.get("DW_AT_name")
    return value.value.decode() if value else None


def type_of(cu_dies, die):
    ref = die.attributes.get("DW_AT_type")
    return cu_dies.get(ref.value + die.cu.cu_offset) if ref else None


def type_name(cu_dies, die, depth=0):
    """Render a DIE's type as a C type name."""
    if die is None or depth > 8:
        return "void"
    tag = die.tag
    if tag == "DW_TAG_base_type":
        return die_name(die) or "?"
    if tag == "DW_TAG_typedef":
        return die_name(die) or "?"
    if tag == "DW_TAG_pointer_type":
        return type_name(cu_dies, type_of(cu_dies, die), depth + 1) + " *"
    if tag == "DW_TAG_const_type":
        return "const " + type_name(cu_dies, type_of(cu_dies, die), depth + 1)
    if tag == "DW_TAG_volatile_type":
        return "volatile " + type_name(cu_dies, type_of(cu_dies, die), depth + 1)
    if tag == "DW_TAG_structure_type":
        return "struct " + (die_name(die) or "<anon>")
    if tag == "DW_TAG_union_type":
        return "union " + (die_name(die) or "<anon>")
    if tag == "DW_TAG_enumeration_type":
        return "enum " + (die_name(die) or "<anon>")
    if tag == "DW_TAG_array_type":
        count = ""
        for child in die.iter_children():
            upper = child.attributes.get("DW_AT_upper_bound")
            if upper is not None:
                count = str(upper.value + 1)
        return f"{type_name(cu_dies, type_of(cu_dies, die), depth + 1)}[{count}]"
    if tag == "DW_TAG_subroutine_type":
        return "func()"
    return tag


def print_aggregate(cu_dies, die, indent=0, base=0):
    pad = " " * indent
    kind = "union" if die.tag == "DW_TAG_union_type" else "struct"
    size = die.attributes.get("DW_AT_byte_size")
    size = size.value if size else "?"
    print(f"{pad}{kind} {die_name(die) or '<anon>'} {{   /* sizeof = {size} */")
    for child in die.iter_children():
        if child.tag != "DW_TAG_member":
            continue
        offset = child.attributes.get("DW_AT_data_member_location")
        offset = (offset.value if offset else 0) + base
        member_type = type_of(cu_dies, child)
        name = die_name(child)
        # Anonymous struct/union members contribute their fields at this
        # offset, which is how QNX spells the st_mtim/st_mtime overlays.
        if name is None and member_type is not None and member_type.tag in (
            "DW_TAG_structure_type",
            "DW_TAG_union_type",
        ):
            print_aggregate(cu_dies, member_type, indent + 4, offset)
            continue
        bits = child.attributes.get("DW_AT_bit_size")
        suffix = f" : {bits.value}" if bits else ""
        print(f"{pad}    /* +{offset:<4} */ {type_name(cu_dies, member_type)} {name}{suffix};")
    print(f"{pad}}};")


def print_enum(die):
    print(f"enum {die_name(die) or '<anon>'} {{")
    for child in die.iter_children():
        if child.tag != "DW_TAG_enumerator":
            continue
        value = child.attributes["DW_AT_const_value"].value
        print(f"    {die_name(child)} = {value},   /* {value:#x} */")
    print("};")


TAGS = {
    "struct": "DW_TAG_structure_type",
    "union": "DW_TAG_union_type",
    "enum": "DW_TAG_enumeration_type",
    "typedef": "DW_TAG_typedef",
    # Finds the (usually anonymous) enum that defines a given enumerator, which
    # is how the _IO_* message numbers and _NTO_* flags are spelled in QNX.
    "const": "DW_TAG_enumerator",
}


def main(argv):
    if len(argv) < 3:
        print(__doc__)
        return 2

    path, requests = argv[1], argv[2:]
    grep = None
    wanted = {}
    for request in requests:
        if request == "--grep":
            continue
        if grep is None and requests[requests.index(request) - 1] == "--grep":
            grep = re.compile(request)
            continue
        kind, _, name = request.partition(":")
        wanted.setdefault(TAGS.get(kind, kind), set()).add(name)

    with open(path, "rb") as handle:
        elf = ELFFile(handle)
        if not elf.has_dwarf_info():
            print(f"{path}: no DWARF; use the SDP's libc.so.5.sym", file=sys.stderr)
            return 1
        dwarf = elf.get_dwarf_info()

        seen = set()
        for cu in dwarf.iter_CUs():
            cu_dies = {die.offset: die for die in cu.iter_DIEs()}
            for die in cu_dies.values():
                name = die_name(die)
                if name is None:
                    continue
                if grep is not None:
                    if not grep.fullmatch(name):
                        continue
                elif name not in wanted.get(die.tag, ()):
                    continue
                if die.attributes.get("DW_AT_declaration"):
                    continue
                key = (die.tag, name)
                if key in seen:
                    continue
                seen.add(key)
                if die.tag in ("DW_TAG_structure_type", "DW_TAG_union_type"):
                    print_aggregate(cu_dies, die)
                elif die.tag == "DW_TAG_enumeration_type":
                    print_enum(die)
                elif die.tag == "DW_TAG_enumerator":
                    print_enum(cu_dies[die.get_parent().offset])
                elif die.tag == "DW_TAG_typedef":
                    print(f"typedef {type_name(cu_dies, type_of(cu_dies, die))} {name};")
                print()
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
