# Maintainer: AmetistYT <ametist0yt@gmail.com>
pkgname=soundpaad-bin
pkgver=0.2.0
pkgrel=1
pkgdesc="Minimalist sound pad with virtual microphone support for Linux (Pure Rust, Kinetix UI)"
arch=('x86_64')
url="https://github.com/ametistik1337/soundpaad"
license=('MIT')
depends=('pipewire-pulse' 'libpulse' 'openssl')
optdepends=('pulseaudio: PulseAudio server (alternative to PipeWire)')
provides=('soundpaad')
conflicts=('soundpaad')
source=("soundpaad-${pkgver}::https://github.com/ametistik1337/soundpaad/releases/download/v${pkgver}/soundpaad"
        "com.soundpaad.app.desktop"
        "com.soundpaad.app.svg")
sha256sums=('SKIP'
            'SKIP'
            'SKIP')

package() {
    install -Dm755 "soundpaad-${pkgver}" "${pkgdir}/usr/bin/soundpaad"
    install -Dm644 "com.soundpaad.app.desktop" "${pkgdir}/usr/share/applications/com.soundpaad.app.desktop"
    install -Dm644 "com.soundpaad.app.svg" "${pkgdir}/usr/share/icons/hicolor/scalable/apps/com.soundpaad.app.svg"
}
