#include <SdFat.h>

#include <driver/gpio.h>
#include <esp_timer.h>
#include <freertos/FreeRTOS.h>
#include <freertos/task.h>

#include <cstdint>
#include <cstring>

namespace {
constexpr uint8_t kSdCsPin = 12;
constexpr uint32_t kSdSpiFrequencyHz = 40000000;
constexpr size_t kMaxNameBytes = 128;
constexpr size_t kMaxEntries = 24;

SdFat sd;
FsFile active_file;
bool active_file_open = false;

struct DirEntry {
  uint8_t kind;
  uint16_t name_len;
  char name[kMaxNameBytes];
};

bool copy_name(DirEntry* out, const char* name, size_t len, uint8_t kind) {
  if (!out || !name) {
    return false;
  }

  const size_t copy_len = (len >= kMaxNameBytes) ? (kMaxNameBytes - 1) : len;
  std::memset(out->name, 0, sizeof(out->name));
  std::memcpy(out->name, name, copy_len);
  out->name[copy_len] = '\0';
  out->name_len = static_cast<uint16_t>(copy_len);
  out->kind = kind;
  return true;
}
}  // namespace

unsigned long millis(void) {
  return static_cast<unsigned long>(esp_timer_get_time() / 1000ULL);
}

void delay(unsigned long ms) {
  vTaskDelay(pdMS_TO_TICKS(ms));
}

void yield(void) {}

void sdCsInit(uint8_t pin) {
  gpio_config_t config = {};
  config.pin_bit_mask = 1ULL << pin;
  config.mode = GPIO_MODE_OUTPUT;
  config.pull_up_en = GPIO_PULLUP_ENABLE;
  config.pull_down_en = GPIO_PULLDOWN_DISABLE;
  config.intr_type = GPIO_INTR_DISABLE;
  gpio_config(&config);
}

void sdCsWrite(uint8_t pin, bool level) {
  gpio_set_level(static_cast<gpio_num_t>(pin), level ? 1 : 0);
}

extern "C" {

struct XteinkSdDirEntry {
  uint8_t kind;
  uint16_t name_len;
  char name[kMaxNameBytes];
};

bool xteink_sd_begin(void) {
  SPI.begin(8, 7, 10, 12);

  const bool ok = sd.begin(kSdCsPin, kSdSpiFrequencyHz);
  return ok;
}

size_t xteink_sd_list_dir(const char* path, XteinkSdDirEntry* out, size_t cap) {
  if (!path || !out || cap == 0) {
    return 0;
  }

  FsFile dir = sd.open(path);
  if (!dir) {
    return 0;
  }

  if (!dir.isDirectory()) {
    dir.close();
    return 0;
  }

  size_t count = 0;
  for (FsFile entry = dir.openNextFile(); entry && count < cap;
       entry = dir.openNextFile()) {
    char name[kMaxNameBytes] = {0};
    const size_t name_len = entry.getName(name, sizeof(name));
    const bool is_dir = entry.isDirectory();
    entry.close();

    if (name_len == 0) {
      continue;
    }
    if (name_len == 1 && name[0] == '.') {
      continue;
    }
    if (name_len == 2 && name[0] == '.' && name[1] == '.') {
      continue;
    }

    auto* dst = reinterpret_cast<DirEntry*>(&out[count]);
    if (!copy_name(dst, name, name_len, is_dir ? 0 : 1)) {
      continue;
    }
    count++;
  }

  dir.close();
  return count;
}

bool xteink_sd_open_file(const char* path) {
  if (!path) {
    return false;
  }

  if (active_file_open) {
    active_file.close();
    active_file_open = false;
  }

  active_file = sd.open(path, O_RDONLY);
  active_file_open = static_cast<bool>(active_file);
  return active_file_open;
}

size_t xteink_sd_file_len(void) {
  if (!active_file_open) {
    return 0;
  }
  return static_cast<size_t>(active_file.fileSize());
}

size_t xteink_sd_read_at(uint64_t offset, uint8_t* buffer, size_t len) {
  if (!active_file_open || !buffer || len == 0) {
    return 0;
  }

  if (!active_file.seekSet(offset)) {
    return 0;
  }

  return active_file.read(buffer, len);
}

void xteink_sd_close_file(void) {
  if (active_file_open) {
    active_file.close();
    active_file_open = false;
  }
}

}  // extern "C"
