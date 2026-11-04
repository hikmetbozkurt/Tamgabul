"""Draws the synthetic images in images/ (invented content, no real data).

File names are camera/screenshot style on purpose: the name must not give
the answer away, so the test measures what OCR and descriptions add.

    python examples/eval/make_images.py

Needs Pillow and the DejaVu fonts. The PNGs are committed; run this only to
change them.
"""
import math
import os

from PIL import Image, ImageDraw, ImageFont

HERE = os.path.join(os.path.dirname(os.path.abspath(__file__)), "images")
FONT = "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"
BOLD = "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf"
MONO = "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf"


def font(size, path=FONT):
    return ImageFont.truetype(path, size)


def save(img, name):
    img.save(os.path.join(HERE, name), optimize=True)


def receipt():
    img = Image.new("RGB", (520, 820), (250, 248, 240))
    d = ImageDraw.Draw(img)
    y = 40
    for text, f in [
        ("BAKKAL ÖRNEK", font(34, BOLD)),
        ("Çiçek Sok. No 4  Kadıköy", font(20)),
        ("Tarih 03.10.2026  14:22", font(20)),
        ("-" * 34, font(22, MONO)),
        ("Ekmek      2 x 15,00   30,00", font(22, MONO)),
        ("Süt        1 x 42,50   42,50", font(22, MONO)),
        ("Domates    1 kg        38,90", font(22, MONO)),
        ("-" * 34, font(22, MONO)),
        ("TOPLAM              111,40 TL", font(24, BOLD)),
        ("NAKİT               120,00", font(22, MONO)),
        ("PARA ÜSTÜ             8,60", font(22, MONO)),
        ("", font(22)),
        ("TEŞEKKÜR EDERİZ", font(26, BOLD)),
    ]:
        d.text((40, y), text, fill=(30, 30, 30), font=f)
        y += 52
    save(img, "IMG_0412.png")


def letter_sideways():
    img = Image.new("RGB", (900, 640), "white")
    d = ImageDraw.Draw(img)
    lines = [
        ("ÖRNEK BANKASI", font(36, BOLD)),
        ("Sayın Müşterimiz,", font(26)),
        ("Kredi kartı ekstreniz hazırdır.", font(26)),
        ("Dönem borcu: 4.250,00 TL", font(26)),
        ("Son ödeme tarihi: 15 Kasım 2026", font(26)),
        ("Saygılarımızla", font(26)),
    ]
    y = 50
    for t, f in lines:
        d.text((60, y), t, fill=(20, 20, 20), font=f)
        y += 85
    # Scanned on its side.
    save(img.rotate(90, expand=True), "scan_0007.png")


def bar_chart():
    img = Image.new("RGB", (900, 600), "white")
    d = ImageDraw.Draw(img)
    d.text((330, 25), "Satışlar", fill=(20, 20, 20), font=font(40, BOLD))
    d.line((100, 520, 840, 520), fill=(60, 60, 60), width=3)
    d.line((100, 100, 100, 520), fill=(60, 60, 60), width=3)
    colors = [(66, 133, 244), (52, 168, 83), (251, 188, 5), (234, 67, 53)]
    for i, (label, h) in enumerate([("Q1", 180), ("Q2", 260), ("Q3", 220), ("Q4", 340)]):
        x = 160 + i * 170
        d.rectangle((x, 520 - h, x + 100, 520), fill=colors[i])
        d.text((x + 30, 535), label, fill=(20, 20, 20), font=font(26))
    save(img, "IMG_0533.png")


def floor_plan():
    img = Image.new("RGB", (900, 700), (255, 255, 255))
    d = ImageDraw.Draw(img)
    w = (30, 30, 30)
    d.rectangle((60, 60, 840, 640), outline=w, width=8)
    d.line((480, 60, 480, 640), fill=w, width=6)
    d.line((60, 380, 480, 380), fill=w, width=6)
    d.line((480, 300, 840, 300), fill=w, width=6)
    # Door gaps.
    d.line((250, 380, 330, 380), fill="white", width=8)
    d.line((480, 450, 480, 530), fill="white", width=8)
    for x, y, t in [(180, 200, "Salon"), (200, 490, "Mutfak"), (560, 160, "Yatak Odası"), (610, 450, "Banyo")]:
        d.text((x, y), t, fill=(40, 40, 40), font=font(30))
    d.text((620, 600), "1:100", fill=(90, 90, 90), font=font(20))
    save(img, "IMG_0601.png")


def boarding_pass():
    img = Image.new("RGB", (1000, 400), (245, 247, 252))
    d = ImageDraw.Draw(img)
    d.rectangle((0, 0, 1000, 70), fill=(20, 60, 140))
    d.text((30, 15), "BOARDING PASS", fill="white", font=font(36, BOLD))
    d.text((40, 110), "IST", fill=(20, 20, 20), font=font(72, BOLD))
    d.text((230, 125), "→", fill=(20, 20, 20), font=font(60))
    d.text((340, 110), "LHR", fill=(20, 20, 20), font=font(72, BOLD))
    for i, (k, v) in enumerate([("FLIGHT", "XY 1980"), ("GATE", "B12"), ("SEAT", "14C"), ("BOARDING", "08:35")]):
        d.text((40 + i * 200, 250), k, fill=(90, 90, 90), font=font(20))
        d.text((40 + i * 200, 280), v, fill=(20, 20, 20), font=font(32, BOLD))
    for i in range(40):  # barcode-like stripes
        if (i * 7) % 3:
            d.rectangle((860 + i * 3, 100, 861 + i * 3, 360), fill=(20, 20, 20))
    save(img, "Screenshot_0318.png")


def error_dialog():
    img = Image.new("RGB", (900, 420), (200, 205, 215))
    d = ImageDraw.Draw(img)
    d.rectangle((100, 60, 800, 360), fill="white", outline=(120, 120, 120), width=2)
    d.rectangle((100, 60, 800, 100), fill=(235, 235, 240))
    d.text((115, 68), "sync-tool.exe", fill=(30, 30, 30), font=font(20))
    d.ellipse((140, 140, 210, 210), fill=(220, 50, 50))
    d.line((158, 158, 192, 192), fill="white", width=7)
    d.line((192, 158, 158, 192), fill="white", width=7)
    d.text((240, 145), "Connection refused", fill=(20, 20, 20), font=font(28, BOLD))
    d.text((240, 190), "(os error 10061) while contacting the server.", fill=(40, 40, 40), font=font(20))
    d.rectangle((650, 290, 770, 335), fill=(0, 103, 192))
    d.text((690, 298), "OK", fill="white", font=font(24))
    save(img, "Screenshot_0402.png")


def cat():
    img = Image.new("RGB", (700, 700), (225, 240, 225))
    d = ImageDraw.Draw(img)
    fur, dark = (240, 150, 60), (60, 40, 30)
    d.ellipse((150, 380, 550, 680), fill=fur)  # body
    d.polygon([(200, 230), (240, 70), (330, 190)], fill=fur)  # ears
    d.polygon([(500, 230), (460, 70), (370, 190)], fill=fur)
    d.ellipse((170, 140, 530, 470), fill=fur)  # head
    d.ellipse((255, 260, 305, 320), fill=dark)
    d.ellipse((395, 260, 445, 320), fill=dark)
    d.polygon([(330, 350), (370, 350), (350, 375)], fill=(230, 100, 120))
    for s in (-1, 1):
        for k in (-20, 0, 20):
            x0 = 350 + s * 40
            d.line((x0, 370 + k / 2, x0 + s * 160, 350 + k * 1.5), fill=dark, width=3)
    for i in range(6):  # tail
        a = i / 5 * math.pi / 2
        d.ellipse((520 + 60 * math.sin(a) - 25, 600 - 120 * a / 2 - 25, 520 + 60 * math.sin(a) + 25, 600 - 120 * a / 2 + 25), fill=fur)
    save(img, "IMG_0719.png")


if __name__ == "__main__":
    os.makedirs(HERE, exist_ok=True)
    for f in (receipt, letter_sideways, bar_chart, floor_plan, boarding_pass, error_dialog, cat):
        f()
