"""Deterministic fallback directory records for unconfigured subscriber IDs."""

from __future__ import annotations

import random
import re
from typing import TypedDict


class GeneratedDirectory(TypedDict):
    directory_id: int
    lines: list[str]


FIRST_NAMES = ("Amina", "Arif", "Bina", "Chandan", "Dalia", "Ehsan", "Farid", "Gita", "Harun", "Ishrat", "Javed", "Kamal", "Laila", "Mahir", "Nabila", "Omar", "Parvin", "Qadir", "Rina", "Sabbir", "Tania", "Umar", "Veda", "Wasim", "Yasmin", "Zahir", "Anika", "Bashir", "Champa", "Dipto", "Elina", "Fahim")
LAST_NAMES = ("Akter", "Barua", "Chowdhury", "Das", "Ghosh", "Haque", "Islam", "Jahan", "Khan", "Miah", "Nandi", "Parvez", "Qureshi", "Rahman", "Sarkar", "Siddique", "Talukder", "Uddin", "Wazed", "Yusuf", "Ahmed", "Bose", "Chakraborty", "Dutta", "Hossain", "Karim", "Mandal", "Roy", "Sen", "Sultana", "Tarafdar", "Zaman")
JOB_TITLES = ("archive clerk", "bicycle mechanic", "bookkeeper", "bus dispatcher", "cafe owner", "cartographer", "clinic nurse", "court translator", "electrical technician", "ferry coordinator", "fish seller", "garden supervisor", "grain inspector", "journalist", "laboratory assistant", "market surveyor", "museum guide", "night watch officer", "postal worker", "radio engineer", "railway planner", "school registrar", "street photographer", "tailor", "telephone technician", "textile designer", "ticket inspector", "town planner", "warehouse manager", "waterworks inspector", "weather observer")
PLACES = ("Aster Court", "Banyan Lane", "Chandni Arcade", "Dolphin Quay", "East Canal Road", "Fountain House", "Green Mango Block", "Harbor View", "Indigo Court", "Jasmine Row", "Kite Market", "Lotus Crossing", "Moonrise Towers", "Nawab Street", "Orchid Estate", "Pineapple Bazaar", "Quiet River Road", "Rupali Square", "Sundarban House", "Tamarind Lane", "Udayan Complex", "Victoria Ghat", "Willow Apartments", "Yellow Brick Court", "Zinnia Gardens", "Ashoka Terrace", "Bridge End", "Crescent Colony", "Dhaka Road", "Ember House", "Fernhill Plaza", "Ganges View")
NOTES = ("keeps a notebook of train times", "repairs an old shortwave radio", "collects postcards from distant towns", "feeds two pigeons at dawn", "knows every shortcut through the market", "brews tea for the whole office", "plays carrom on Friday evenings", "maintains a small rooftop garden", "writes letters with a fountain pen", "has a bicycle named Neel", "saves newspaper clippings", "volunteers at the neighborhood clinic", "carries a red umbrella year-round", "teaches chess to local children", "keeps a map of the riverbank", "raises jasmine beside the front door")

_ID_PATTERN = re.compile(r"SUBSCRIBER ID\s+(\d{4})", re.IGNORECASE)


def generated_directory_page(directory_id: int) -> GeneratedDirectory | None:
    if not 0 <= directory_id <= 9999:
        return None
    rng = random.Random(directory_id)
    if rng.random() >= 0.70:
        return None
    return {
        "directory_id": directory_id,
        "lines": [
            f"SUBSCRIBER ID {directory_id:04}",
            f"NAME // {rng.choice(FIRST_NAMES)} {rng.choice(LAST_NAMES)}",
            f"OCCUPATION // {rng.choice(JOB_TITLES)}",
            f"NOTE // {rng.choice(NOTES)}",
            f"LOCATION // {rng.choice(PLACES)}",
        ],
    }


def unknown_id(lines: object) -> int | None:
    if not isinstance(lines, list):
        return None
    for line in lines:
        match = _ID_PATTERN.fullmatch(str(line).strip())
        if match:
            return int(match.group(1))
    return None
