#!/usr/bin/env python3
"""Validate esphome-relay/config.yaml against the HA Supervisor schema.

Downloads the actual validation patterns from the HA Supervisor repo at runtime,
so we always validate against the latest rules without maintaining duplicates.

Sources:
  supervisor/addons/validate.py  → RE_VOLUME, _SCHEMA_ADDON_CONFIG structure
  supervisor/addons/options.py   → RE_SCHEMA_ELEMENT
  supervisor/addons/const.py     → MappingType, RE_SLUG

Requires: pip install voluptuous pyyaml
"""

import re
import sys
import urllib.request
from pathlib import Path
from typing import List

import yaml

SUPERVISOR_RAW = (
    "https://raw.githubusercontent.com/home-assistant/supervisor/main/supervisor"
)


def fetch(path: str) -> str:
    """Fetch a file from the HA Supervisor main branch."""
    url = f"{SUPERVISOR_RAW}/{path}"
    with urllib.request.urlopen(url, timeout=15) as resp:
        return resp.read().decode()


def extract_regex(source, name):
    """Extract a compiled regex from Python source by variable name."""
    # Matches both single-line and multi-line re.compile(...) assignments
    pattern = rf'{name}\s*=\s*re\.compile\(\s*(.*?)\)\s*\)'
    # For multi-line patterns, grab everything between re.compile( and the closing )
    # Use a simpler approach: find the re.compile call and extract the raw string
    idx = source.find(f"{name} = re.compile(")
    if idx == -1:
        idx = source.find(f"{name} =re.compile(")
    if idx == -1:
        raise ValueError(f"Could not find {name} in source")

    # Find the balanced closing paren of re.compile(...)
    start = source.index("re.compile(", idx) + len("re.compile(")
    depth = 1
    pos = start
    while depth > 0 and pos < len(source):
        if source[pos] == "(":
            depth += 1
        elif source[pos] == ")":
            depth -= 1
        pos += 1
    raw_arg = source[start : pos - 1].strip()

    # Evaluate the string concatenation safely
    # The patterns are always raw string literals concatenated with +
    parts = re.findall(r'r"(.*?)"', raw_arg, re.DOTALL)
    if not parts:
        parts = re.findall(r"r'(.*?)'", raw_arg, re.DOTALL)
    combined = "".join(parts)
    return re.compile(combined)


def extract_strenum_values(source, class_name):
    """Extract values from a StrEnum class definition."""
    # Find the class block
    idx = source.find(f"class {class_name}(StrEnum):")
    if idx == -1:
        raise ValueError(f"Could not find {class_name} in source")

    # Take only the class body: stop at next top-level class/def or end-of-file
    block = source[idx:]
    # Skip the class line itself, then find next unindented class/def
    match = re.search(r'\n(?=[A-Z@]|class |def )', block[1:])
    if match:
        block = block[:match.start() + 1]
    values = re.findall(r'=\s*"([^"]+)"', block)
    return values


def extract_list(source, name):
    """Extract a list of strings from Python source, resolving constant references."""
    idx = source.find(f"{name} =")
    if idx == -1:
        idx = source.find(f"{name}=")
    if idx == -1:
        raise ValueError(f"Could not find list {name}")

    block = source[idx:]
    end = block.index("]") + 1
    list_text = block[:end]

    # First try direct string literals
    values = re.findall(r'"([^"]+)"', list_text)
    if values:
        return values

    # Otherwise resolve constant references (e.g. ARCH_AARCH64)
    refs = re.findall(r'\b([A-Z_][A-Z_0-9]+)\b', list_text.split("=", 1)[1])
    resolved = []
    for ref in refs:
        m = re.search(rf'{ref}\s*=\s*"([^"]+)"', source)
        if m:
            resolved.append(m.group(1))
    return resolved


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------
def main():
    config_path = (
        Path(__file__).resolve().parent.parent / "esphome-relay" / "config.yaml"
    )
    if not config_path.exists():
        print(f"ERROR: {config_path} not found")
        return 1

    with open(config_path) as f:
        raw = yaml.safe_load(f)

    # --- Download sources from HA Supervisor ---
    print("Fetching validation rules from HA Supervisor repo...")
    try:
        validate_src = fetch("addons/validate.py")
        options_src = fetch("addons/options.py")
        const_src = fetch("addons/const.py")
        main_const_src = fetch("const.py")
    except Exception as e:
        print(f"ERROR: Could not fetch HA Supervisor sources: {e}")
        return 1

    # --- Extract patterns ---
    re_volume = extract_regex(validate_src, "RE_VOLUME")
    re_schema = extract_regex(options_src, "RE_SCHEMA_ELEMENT")
    mapping_types = extract_strenum_values(const_src, "MappingType")
    arch_all = extract_list(main_const_src, "ARCH_ALL")
    arch_deprecated = extract_list(main_const_src, "ARCH_DEPRECATED")
    arch_all_compat = arch_all + arch_deprecated
    print(f"  RE_VOLUME: {re_volume.pattern}")
    print(f"  RE_SCHEMA_ELEMENT: {re_schema.pattern[:80]}...")
    print(f"  MappingType values: {mapping_types}")
    print(f"  ARCH_ALL_COMPAT: {arch_all_compat}")

    # --- Build validators from extracted patterns ---
    errors = []  # type: List[str]

    # Required top-level keys
    for key in ["name", "version", "slug", "description", "arch"]:
        if key not in raw:
            errors.append(f"Missing required key: {key}")

    # slug format
    if "slug" in raw and not re.match(r"^[-_.A-Za-z0-9]+$", str(raw["slug"])):
        errors.append(f"Invalid slug: {raw['slug']}")

    # arch values
    for arch in raw.get("arch", []):
        if arch not in arch_all_compat:
            errors.append(f"Invalid architecture '{arch}', valid: {arch_all_compat}")

    # map entries
    for entry in raw.get("map", []):
        if isinstance(entry, str):
            if not re_volume.match(entry):
                errors.append(
                    f"Invalid map entry '{entry}'. "
                    f"Valid types: {', '.join(mapping_types)}"
                )
        elif isinstance(entry, dict):
            if "type" not in entry:
                errors.append(f"Map dict entry missing 'type': {entry}")
            elif entry["type"] not in mapping_types:
                errors.append(f"Invalid map type: {entry['type']}")

    # schema types
    for key, val in raw.get("schema", {}).items():
        if isinstance(val, str) and not re_schema.match(val):
            errors.append(
                f"Invalid schema type for '{key}': '{val}' "
                f"(does not match HA RE_SCHEMA_ELEMENT)"
            )

    # startup
    valid_startup = ["initialize", "system", "services", "application", "once"]
    if raw.get("startup") and raw["startup"] not in valid_startup:
        errors.append(f"Invalid startup: {raw['startup']}, valid: {valid_startup}")

    # boot
    valid_boot = ["auto", "manual"]
    if raw.get("boot") and raw["boot"] not in valid_boot:
        errors.append(f"Invalid boot: {raw['boot']}, valid: {valid_boot}")

    # ports format
    for port_key in raw.get("ports", {}):
        if not re.match(r"^\d+/(tcp|udp)$", str(port_key)):
            errors.append(f"Invalid port key '{port_key}', expected '<port>/(tcp|udp)'")

    # options/schema consistency
    options = raw.get("options", {})
    schema = raw.get("schema", {})
    if isinstance(schema, dict):
        for key in options:
            if key not in schema:
                errors.append(f"Option '{key}' has no matching schema entry")
        for key, val in schema.items():
            is_optional = isinstance(val, str) and val.endswith("?")
            if not is_optional and key not in options:
                errors.append(
                    f"Required schema key '{key}' has no default in options"
                )

    if errors:
        print(f"\nVALIDATION FAILED ({len(errors)} errors):\n")
        for err in errors:
            print(f"  ✗ {err}")
        return 1

    print(f"\n✓ config.yaml is valid (version: {raw.get('version', '?')})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
