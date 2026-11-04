include $(TOPDIR)/rules.mk

PKG_NAME:=nyanpasu
PKG_VERSION:=0.0.0
PKG_RELEASE:=1

# POC: built from a source checkout of this repository, for example with
# `make package/nyanpasu/compile` after pointing the feed at it. Pin a release
# tag and PKG_HASH before publishing to a feed.
PKG_SOURCE_PROTO:=git
PKG_SOURCE_URL:=https://github.com/libnyanpasu/clash-nyanpasu.git
PKG_SOURCE_VERSION:=HEAD
PKG_MIRROR_HASH:=skip

PKG_LICENSE:=GPL-3.0-or-later
PKG_LICENSE_FILES:=LICENSE
PKG_MAINTAINER:=Nyanpasu maintainers <noreply@example.invalid>

PKG_BUILD_DEPENDS:=rust/host
PKG_BUILD_PARALLEL:=1
PKG_BUILD_FLAGS:=no-mips16

# Build from the lockfile of the backend workspace.
CARGO_PKG_ARGS:=--locked

include $(INCLUDE_DIR)/package.mk
include $(TOPDIR)/feeds/packages/lang/rust/rust-package.mk

define Package/nyanpasu
  SECTION:=net
  CATEGORY:=Network
  SUBMENU:=Web Servers/Proxies
  TITLE:=Nyanpasu server and CLI
  URL:=https://github.com/libnyanpasu/clash-nyanpasu
  DEPENDS:=$(RUST_ARCH_DEPENDS) +ca-bundle
endef

define Package/nyanpasu/description
  nyanpasu-server owns the Nyanpasu application and the Mihomo core and serves a
  local control socket; nyanpasu-cli talks to it, and luci-app-nyanpasu drives
  the CLI. The Mihomo core binary is not part of this package: it is placed at
  <data-dir>/mihomo.
endef

define Package/nyanpasu/conffiles
/etc/nyanpasu
endef

define Build/Compile
	$(call Build/Compile/Cargo,backend/nyanpasu-server)
	$(call Build/Compile/Cargo,backend/nyanpasu-cli)
endef

define Package/nyanpasu/install
	$(INSTALL_DIR) $(1)/usr/bin
	$(INSTALL_BIN) $(PKG_INSTALL_DIR)/bin/nyanpasu-server $(1)/usr/bin/nyanpasu-server
	$(INSTALL_BIN) $(PKG_INSTALL_DIR)/bin/nyanpasu-cli $(1)/usr/bin/nyanpasu-cli
	$(INSTALL_DIR) $(1)/etc/init.d
	$(INSTALL_BIN) ./files/nyanpasu.init $(1)/etc/init.d/nyanpasu
	$(INSTALL_DIR) $(1)/etc/nyanpasu
	$(INSTALL_DIR) $(1)/usr/share/nyanpasu
endef

$(eval $(call BuildPackage,nyanpasu))
