# pins.py
#
# The pin mapping and pin definitions

__all__ = ["get_pin", "pin_map", "pio_a", "pio_b", "pio_c", "pio_d", "pio_e", "pios"]


pio_a = {
    "PA0": [102, 0x00],
    "PA1": [99, 0x01],
    "PA2": [93, 0x02],
    "PA3": [91, 0x03],
    "PA4": [77, 0x04],
    "PA5": [73, 0x05],
    "PA6": [114, 0x06],
    "PA7": [35, 0x07],
    "PA8": [36, 0x08],
    "PA9": [75, 0x09],
    "PA10": [66, 0x0A],
    "PA11": [64, 0x0B],
    "PA12": [68, 0x0C],
    "PA13": [42, 0x0D],
    "PA14": [51, 0x0E],
    "PA15": [49, 0x0F],
    "PA16": [45, 0x10],
    "PA17": [25, 0x11],
    "PA18": [24, 0x12],
    "PA19": [23, 0x13],
    "PA20": [22, 0x14],
    "PA21": [32, 0x15],
    "PA22": [37, 0x16],
    "PA23": [46, 0x17],
    "PA24": [56, 0x18],
    "PA25": [59, 0x19],
    "PA26": [62, 0x1A],
    "PA27": [70, 0x1B],
    "PA28": [112, 0x1C],
    "PA29": [129, 0x1D],
    "PA30": [116, 0x1E],
    "PA31": [118, 0x1F],
}

pio_b = {
    "PB0": [21, 0x20],
    "PB1": [20, 0x21],
    "PB2": [26, 0x22],
    "PB3": [31, 0x23],
    "PB4": [105, 0x24],
    "PB5": [109, 0x25],
    "PB6": [79, 0x26],
    "PB7": [89, 0x27],
    "PB8": [141, 0x28],
    "PB9": [142, 0x29],
    "PB10": [136, 0x2A],
    "PB11": [137, 0x2B],
    "PB12": [87, 0x2C],
    "PB13": [144, 0x2D],
    "PB14": [140, 0x2E],
}

pio_c = {
    "PC0": [11, 0x2F],
    "PC1": [38, 0x30],
    "PC2": [39, 0x31],
    "PC3": [49, 0x32],
    "PC4": [41, 0x33],
    "PC5": [58, 0x34],
    "PC6": [54, 0x35],
    "PC7": [48, 0x36],
    "PC8": [82, 0x37],
    "PC9": [86, 0x38],
    "PC10": [90, 0x39],
    "PC11": [94, 0x3A],
    "PC12": [17, 0x3B],
    "PC13": [19, 0x3C],
    "PC14": [97, 0x3D],
    "PC15": [18, 0x3E],
    "PC16": [100, 0x3F],
    "PC17": [103, 0x40],
    "PC18": [111, 0x41],
    "PC19": [117, 0x42],
    "PC20": [120, 0x43],
    "PC21": [121, 0x44],
    "PC22": [124, 0x45],
    "PC23": [127, 0x46],
    "PC24": [130, 0x47],
    "PC25": [133, 0x48],
    "PC26": [13, 0x49],
    "PC27": [12, 0x4A],
    "PC28": [76, 0x4B],
    "PC29": [16, 0x4C],
    "PC30": [15, 0x4D],
    "PC31": [14, 0x4E],
}

pio_d = {
    "PD0": [1, 0x4F],
    "PD1": [132, 0x50],
    "PD2": [131, 0x51],
    "PD3": [128, 0x52],
    "PD4": [126, 0x53],
    "PD5": [125, 0x54],
    "PD6": [121, 0x55],
    "PD7": [119, 0x56],
    "PD8": [113, 0x57],
    "PD9": [110, 0x58],
    "PD10": [101, 0x59],
    "PD11": [98, 0x5A],
    "PD12": [92, 0x5B],
    "PD13": [88, 0x5C],
    "PD14": [84, 0x5D],
    "PD15": [106, 0x5E],
    "PD16": [78, 0x5F],
    "PD17": [74, 0x60],
    "PD18": [69, 0x61],
    "PD19": [67, 0x62],
    "PD20": [65, 0x63],
    "PD21": [63, 0x64],
    "PD22": [60, 0x65],
    "PD23": [57, 0x66],
    "PD24": [55, 0x67],
    "PD25": [52, 0x68],
    "PD26": [53, 0x69],
    "PD27": [47, 0x6A],
    "PD28": [71, 0x6B],
    "PD29": [108, 0x6C],
    "PD30": [34, 0x6D],
    "PD31": [2, 0x6E],
}

pio_e = {
    "PE0": [4, 0x6F],
    "PE1": [6, 0x70],
    "PE2": [7, 0x71],
    "PE3": [10, 0x72],
    "PE4": [27, 0x73],
    "PE5": [28, 0x74],
}

pios = {
    "PIOA": pio_a,
    "PIOB": pio_b,
    "PIOC": pio_c,
    "PIOD": pio_d,
    "PIOE": pio_e,
}


pin_map = {pin: num for pio in pios.values() for pin, num in pio.items()}

pin_id_map = {
    data[1]: (name, data[0]) for pio in pios.values() for name, data in pio.items()
}


def get_pin(pin_id: int) -> tuple[str, int] | None:
    """
    Find a pin by its hex ID (e.g. 0x2A).
    Returns (pin_name, pin_number) or None if not found.
    """
    return pin_id_map.get(pin_id)


if __name__ == "__main__":
    print("pio banks count:", len(pios))
    print("pio banks:", list(pios.keys()))
    print("PD10:", pin_map["PD10"])
    print("PC4:", pin_map["PC4"])
    print("PA5 pin:", pin_map["PA5"][0])
    print("PE4 hex:", hex(pin_map["PE4"][1]))
