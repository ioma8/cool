#include "SPI.h"

#include <cstring>

#include <esp_err.h>

namespace {
constexpr uint32_t kDefaultFreq = 1000000;

spi_host_device_t host_for_bus(uint8_t bus) {
  (void)bus;
  return SPI2_HOST;
}
}  // namespace

SPIClass::SPIClass(uint8_t spi_bus)
    : _spi_num(spi_bus),
      _sck(-1),
      _miso(-1),
      _mosi(-1),
      _ss(-1),
      _freq(kDefaultFreq),
      _bitOrder(kSpiBitOrderMsbFirst),
      _dataMode(kSpiDataMode0),
      _inTransaction(false),
      _busInitialized(false),
      _host(host_for_bus(spi_bus)),
      _device(nullptr) {}

SPIClass::~SPIClass() {
  end();
}

bool SPIClass::begin(int8_t sck, int8_t miso, int8_t mosi, int8_t ss) {
  if (_busInitialized) {
    return true;
  }

  _sck = sck;
  _miso = miso;
  _mosi = mosi;
  _ss = ss;

  spi_bus_config_t buscfg = {};
  buscfg.miso_io_num = miso;
  buscfg.mosi_io_num = mosi;
  buscfg.sclk_io_num = sck;
  buscfg.quadwp_io_num = -1;
  buscfg.quadhd_io_num = -1;
  buscfg.max_transfer_sz = 4096;

  if (spi_bus_initialize(_host, &buscfg, SPI_DMA_CH_AUTO) != ESP_OK) {
    return false;
  }

  _busInitialized = true;
  rebuild_device();
  return _device != nullptr;
}

void SPIClass::end() {
  destroy_device();
  if (_busInitialized) {
    spi_bus_free(_host);
    _busInitialized = false;
  }
}

void SPIClass::setHwCs(bool) {}

void SPIClass::setSSInvert(bool) {}

void SPIClass::setBitOrder(uint8_t bitOrder) {
  _bitOrder = bitOrder;
  if (_device) {
    rebuild_device();
  }
}

void SPIClass::setDataMode(uint8_t dataMode) {
  _dataMode = dataMode;
  if (_device) {
    rebuild_device();
  }
}

void SPIClass::setFrequency(uint32_t freq) {
  _freq = freq;
  if (_device) {
    rebuild_device();
  }
}

void SPIClass::setClockDivider(uint32_t clockDiv) {
  _freq = clockDiv;
  if (_device) {
    rebuild_device();
  }
}

uint32_t SPIClass::getClockDivider() {
  return _freq;
}

void SPIClass::beginTransaction(SPISettings settings) {
  _freq = settings._clock;
  _bitOrder = settings._bitOrder;
  _dataMode = settings._dataMode;
  if (!_busInitialized) {
    begin(_sck, _miso, _mosi, _ss);
  } else {
    rebuild_device();
  }
  _inTransaction = true;
}

void SPIClass::endTransaction(void) {
  _inTransaction = false;
}

void SPIClass::transfer(void* data, uint32_t size) {
  transferBytes(static_cast<const uint8_t*>(data), static_cast<uint8_t*>(data), size);
}

void SPIClass::transfer(const uint8_t* data, uint8_t* out, uint32_t size) {
  transferBytes(data, out, size);
}

uint8_t SPIClass::transfer(uint8_t data) {
  uint8_t out = 0;
  transferBytes(&data, &out, 1);
  return out;
}

uint16_t SPIClass::transfer16(uint16_t data) {
  uint16_t out = 0;
  transferBytes(reinterpret_cast<const uint8_t*>(&data), reinterpret_cast<uint8_t*>(&out), sizeof(out));
  return out;
}

uint32_t SPIClass::transfer32(uint32_t data) {
  uint32_t out = 0;
  transferBytes(reinterpret_cast<const uint8_t*>(&data), reinterpret_cast<uint8_t*>(&out), sizeof(out));
  return out;
}

void SPIClass::transferBytes(const uint8_t* data, uint8_t* out, uint32_t size) {
  if (!_device || size == 0) {
    return;
  }

  constexpr size_t kChunk = 64;
  uint8_t tx[kChunk];
  uint8_t rx[kChunk];

  while (size > 0) {
    const uint32_t n = size > kChunk ? kChunk : size;
    if (data) {
      std::memcpy(tx, data, n);
    } else {
      std::memset(tx, 0xFF, n);
    }

    spi_transaction_t t = {};
    t.length = n * 8;
    t.tx_buffer = tx;
    t.rx_buffer = rx;
    if (spi_device_polling_transmit(_device, &t) != ESP_OK) {
      return;
    }

    if (out) {
      std::memcpy(out, rx, n);
    }

    if (data) {
      data += n;
    }
    if (out) {
      out += n;
    }
    size -= n;
  }
}

void SPIClass::transferBits(uint32_t data, uint32_t* out, uint8_t bits) {
  uint32_t rx = 0;
  transferBytes(reinterpret_cast<const uint8_t*>(&data), reinterpret_cast<uint8_t*>(&rx), (bits + 7) / 8);
  if (out) {
    *out = rx;
  }
}

void SPIClass::write(uint8_t data) {
  transfer(data);
}

void SPIClass::write16(uint16_t data) {
  transfer16(data);
}

void SPIClass::write32(uint32_t data) {
  transfer32(data);
}

void SPIClass::writeBytes(const uint8_t* data, uint32_t size) {
  transferBytes(data, nullptr, size);
}

void SPIClass::writePixels(const void* data, uint32_t size) {
  transferBytes(static_cast<const uint8_t*>(data), nullptr, size);
}

void SPIClass::writePattern(const uint8_t* data, uint8_t size, uint32_t repeat) {
  while (repeat--) {
    writeBytes(data, size);
  }
}

void SPIClass::ensure_bus() {
  if (!_busInitialized) {
    begin(_sck, _miso, _mosi, _ss);
  }
}

void SPIClass::rebuild_device() {
  destroy_device();
  ensure_bus();
  if (!_busInitialized) {
    return;
  }

  spi_device_interface_config_t devcfg = {};
  devcfg.clock_speed_hz = _freq;
  devcfg.mode = _dataMode;
  devcfg.spics_io_num = -1;
  devcfg.queue_size = 1;
  devcfg.flags = SPI_DEVICE_NO_DUMMY;

  if (spi_bus_add_device(_host, &devcfg, &_device) != ESP_OK) {
    _device = nullptr;
  }
}

void SPIClass::destroy_device() {
  if (_device) {
    spi_bus_remove_device(_device);
    _device = nullptr;
  }
}

#if !defined(NO_GLOBAL_INSTANCES) && !defined(NO_GLOBAL_SPI)
SPIClass SPI;
#endif
