import json
import logging
import os
import sys
from textwrap import dedent

import Utils
from Options import (
    Choice,
    FreeText,
    NamedRange,
    Option,
    OptionCounter,
    OptionList,
    OptionSet,
    Range,
    TextChoice,
    Toggle,
    VerifyKeys,
)
from worlds import AutoWorldRegister
from worlds.AutoWorld import World


# from OptionsCreator.py, but don't import all the kvui stuff
def option_can_be_randomized(option: type[Option]):
    # most options can be randomized, so we should just check for those that cannot
    if not option.supports_weighting:
        return False
    if issubclass(option, FreeText) and not issubclass(option, TextChoice):
        return False
    return True


def create_type(option: type[Option], world: type[World]):
    if issubclass(option, NamedRange):
        return {
            "type": "NamedRange",
            "default": option.default,
            "min": option.range_start,
            "max": option.range_end,
            "presets": option.special_range_names,
        }
    if issubclass(option, Range):
        return {
            "type": "Range",
            "default": option.default,
            "min": option.range_start,
            "max": option.range_end,
        }
    if issubclass(option, Toggle):
        return {"type": "Toggle", "default": bool(option.default)}
    if issubclass(option, TextChoice):
        # TODO: is choices good like this?
        return {
            "type": "TextChoice",
            "default": option.default,
            "choices": [
                {"name": choice, "display_name": option.get_option_name(val), "value": val}
                for val, choice in option.name_lookup.items()
            ],
        }
    if issubclass(option, Choice):
        return {
            "type": "Choice",
            "default": option.default,
            "choices": [
                {"name": choice, "display_name": option.get_option_name(val), "value": val}
                for val, choice in option.name_lookup.items()
            ],
        }
    if issubclass(option, FreeText):
        return {"type": "FreeText", "default": option.default}
    if issubclass(option, VerifyKeys):
        valid_keys = sorted(option.valid_keys)
        if option.verify_item_name:
            valid_keys += list(world.item_name_to_id.keys())
            if option.convert_name_groups:
                valid_keys += list(world.item_name_groups.keys())
        if option.verify_location_name:
            valid_keys += list(world.location_name_to_id.keys())
            if option.convert_name_groups:
                valid_keys += list(world.location_name_groups.keys())

        if issubclass(option, OptionSet) and valid_keys:
            return {"type": "OptionSet", "default": list(option.default), "options": valid_keys}
        if issubclass(option, OptionList) and valid_keys:
            return {"type": "OptionList", "default": list(option.default), "options": valid_keys}
        if issubclass(option, OptionCounter) and valid_keys:
            return {
                "type": "OptionCounter",
                "default": dict(option.default),
                "min": option.min,
                "max": option.max,
                "options": valid_keys,
            }
        # if issubclass(option, OptionDict):
        #     return {"type": "OptionDict", "default": dict(option.default), "options": valid_keys}
    return {"type": "Unknown"}


def generate_for_world(world_name: str, world: type[World]) -> dict:
    group_names = ["Game Options", *(group.name for group in world.web.option_groups)]
    groups = {name: [] for name in group_names}
    for name, option in world.options_dataclass.type_hints.items():
        group = next((group.name for group in world.web.option_groups if option in group.options), "Game Options")
        groups[group].append((name, option))

    return {
        "name": world_name,
        "game": world.game,
        "version": world.world_version.as_simple_string(),
        "hidden": world.hidden,
        "groups": [
            {
                "name": group,
                "options": [
                    {
                        "name": name,
                        "display_name": getattr(option, "display_name", name),
                        "description": dedent(option.__doc__ or "").strip(),
                        "randomizable": option_can_be_randomized(option),
                        "visibility": option.visibility,
                        **create_type(option, world),
                    }
                    for name, option in options
                ],
                "collapsed": next((g.start_collapsed for g in world.web.option_groups if g.name == group), False),
            }
            for group, options in groups.items()
        ],
    }


def main():
    Utils.init_logging("GenerateSchema", loglevel="info")
    os.makedirs("schema", exist_ok=True)
    filter = sys.argv[1:]
    for world in AutoWorldRegister.world_types.values():
        name = world.__module__.rsplit(".")[1]
        if filter != [] and name not in filter:
            continue
        logging.info(f"generating schema for {name}")
        with open(f"schema/{name}.json", "w") as file:
            json.dump(generate_for_world(name, world), file)


if __name__ == "__main__":
    main()
