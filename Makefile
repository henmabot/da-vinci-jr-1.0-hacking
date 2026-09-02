TARGET := firmware
BUILD_DIR := build

CROSS_COMPILE ?= arm-none-eabi-
CC := $(CROSS_COMPILE)gcc
OBJCOPY := $(CROSS_COMPILE)objcopy
SIZE := $(CROSS_COMPILE)size

CPU_FLAGS := -mcpu=cortex-m4 -mthumb -mfloat-abi=soft
CPPFLAGS := -Iinclude \
	-Ivendor/sam4e/include \
	-Ivendor/cmsis-core
CFLAGS := $(CPU_FLAGS) -std=gnu11 -ffreestanding -Os -g3 -Wall -Wextra -Werror \
	-ffunction-sections -fdata-sections -fno-common \
	-fno-unwind-tables -fno-asynchronous-unwind-tables -MMD -MP

LDSCRIPT := linker/sam4e8e.ld
LDFLAGS := $(CPU_FLAGS) -nostdlib -T$(LDSCRIPT) \
	-Wl,--gc-sections -Wl,--build-id=none \
	-Wl,-Map,$(BUILD_DIR)/$(TARGET).map
LDLIBS := -lgcc

SRCS := \
	src/main.c \
	platform/startup.c \
	platform/clock.c \
	platform/gpio.c \
	platform/usb_cdc.c

OBJS := $(SRCS:%.c=$(BUILD_DIR)/%.o)
DEPS := $(OBJS:.o=.d)

ELF := $(BUILD_DIR)/$(TARGET).elf
BIN := $(BUILD_DIR)/$(TARGET).bin
all: $(ELF) $(BIN)
	$(SIZE) $(ELF)

$(ELF): $(OBJS) $(LDSCRIPT)
	@mkdir -p $(@D)
	$(CC) $(LDFLAGS) $(OBJS) $(LDLIBS) -o $@

$(BIN): $(ELF)
	$(OBJCOPY) -O binary $< $@

$(BUILD_DIR)/%.o: %.c
	@mkdir -p $(@D)
	$(CC) $(CPPFLAGS) $(CFLAGS) -c $< -o $@

clean:
	rm -rf $(BUILD_DIR)

-include $(DEPS)

.PHONY: all clean
