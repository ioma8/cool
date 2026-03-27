#pragma once

#include <cstdint>

#include <driver/spi_master.h>

#define SPI_HAS_TRANSACTION

static constexpr uint8_t kSpiBitOrderMsbFirst = 0;
static constexpr uint8_t kSpiDataMode0 = 0;
static constexpr uint8_t kSpiDefaultBus = 2;

class SPISettings {
 public:
  SPISettings() : _clock(1000000), _bitOrder(kSpiBitOrderMsbFirst), _dataMode(kSpiDataMode0) {}
  SPISettings(uint32_t clock, uint8_t bitOrder, uint8_t dataMode)
      : _clock(clock), _bitOrder(bitOrder), _dataMode(dataMode) {}

  uint32_t _clock;
  uint8_t _bitOrder;
  uint8_t _dataMode;
};

class SPIClass {
 public:
  explicit SPIClass(uint8_t spi_bus = kSpiDefaultBus);
  ~SPIClass();

  bool begin(int8_t sck = -1, int8_t miso = -1, int8_t mosi = -1, int8_t ss = -1);
  void end();

  void setHwCs(bool use);
  void setSSInvert(bool invert);
  void setBitOrder(uint8_t bitOrder);
  void setDataMode(uint8_t dataMode);
  void setFrequency(uint32_t freq);
  void setClockDivider(uint32_t clockDiv);

  uint32_t getClockDivider();

  void beginTransaction(SPISettings settings);
  void endTransaction(void);
  void transfer(void* data, uint32_t size);
  void transfer(const uint8_t* data, uint8_t* out, uint32_t size);
  uint8_t transfer(uint8_t data);
  uint16_t transfer16(uint16_t data);
  uint32_t transfer32(uint32_t data);

  void transferBytes(const uint8_t* data, uint8_t* out, uint32_t size);
  void transferBits(uint32_t data, uint32_t* out, uint8_t bits);

  void write(uint8_t data);
  void write16(uint16_t data);
  void write32(uint32_t data);
  void writeBytes(const uint8_t* data, uint32_t size);
  void writePixels(const void* data, uint32_t size);
  void writePattern(const uint8_t* data, uint8_t size, uint32_t repeat);

  spi_device_handle_t bus() { return _device; }
  int8_t pinSS() { return _ss; }

 private:
  void ensure_bus();
  void rebuild_device();
  void destroy_device();

  uint8_t _spi_num;
  int8_t _sck;
  int8_t _miso;
  int8_t _mosi;
  int8_t _ss;
  uint32_t _freq;
  uint8_t _bitOrder;
  uint8_t _dataMode;
  bool _inTransaction;
  bool _busInitialized;
  spi_host_device_t _host;
  spi_device_handle_t _device;
};

#if !defined(NO_GLOBAL_INSTANCES) && !defined(NO_GLOBAL_SPI)
extern SPIClass SPI;
#endif
