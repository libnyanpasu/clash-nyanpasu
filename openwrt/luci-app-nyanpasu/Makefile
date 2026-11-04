include $(TOPDIR)/rules.mk

LUCI_TITLE:=LuCI support for Nyanpasu
LUCI_DEPENDS:=+nyanpasu +rpcd-mod-ucode +luci-base
LUCI_PKGARCH:=all

PKG_LICENSE:=GPL-3.0-or-later
PKG_MAINTAINER:=Nyanpasu maintainers <noreply@example.invalid>

# The directory is meant to be dropped into a luci feed (applications/), where
# luci.mk is two levels up.
include ../../luci.mk

# call BuildPackage - OpenWrt buildroot signature
