# Tekstur vintage (pustaka bawaan)

Setiap file gambar di folder ini otomatis ikut ke aplikasi StudioZea saat dibuild
(`engine/build.rs` menanamkannya ke program). Di aplikasi, tekstur muncul di kartu
**5. Tekstur vintage** dengan nama file sebagai namanya (`vintage-03.jpg` → "vintage 03").

## Menambah tekstur

1. Buka folder `textures/` di GitHub (repo ini) → **Add file → Upload files**.
2. Upload file PNG, JPG, TIFF (juga TIFF bitmap 1-bit), WebP atau BMP.
3. **Commit** ke branch yang dibuild (sekarang `claude/dtf-halftone-design-hkpios`).
4. Buka tab **Actions → Build Windows app → Run workflow** (pilih branch yang sama).
   GitHub Actions membuat installer baru (± 15 menit). Download dari tab **Releases**:
   tekstur baru sudah ada di daftar. (Commit saja tidak lagi membuat installer.)

Untuk menghapus tekstur: hapus filenya dari folder ini lalu commit.

## Format tekstur

- **Hitam = tinta aus/hilang, putih = tinta tetap.** Abu-abu = sebagian (dipakai oleh
  mode *Pudar* dan *Campur*, dan oleh slider *Kekuatan*).
- PNG transparan juga bisa: bagian yang **tidak transparan** dianggap aus.
- Resolusi: makin besar makin tajam. Tekstur menutupi seluruh desain, jadi untuk cetak
  30 cm @300 DPI idealnya lebar ± 3500 px (contoh di sini 1158 px masih bagus karena
  retakannya lebih besar dari 0,15 mm). Hindari file di atas ± 20 MB supaya installer
  tetap kecil.
- Nama file pakai huruf kecil dan tanda `-`, misalnya `plastisol-retak-01.png`.

## Cara aplikasi memakai tekstur

- Tekstur diperbesar sampai **menutupi seluruh cetakan** (rasio tetap, di tengah), lalu
  bisa diputar 90°, dicermin, digeser, dan diperbesar/diperkecil (di luar tekstur
  diulang secara cermin, tanpa sambungan terlihat).
- **Lubang**: area gelap jadi lubang di tinta. Titik halftone dibuang/dipertahankan
  utuh (bukan terpotong), area solid dipotong rapi; titik dan lubang tetap ≥ 0,15 mm.
- **Campur**: lubang di area gelap + nada memudar di area abu-abu (paling alami).
- **Pudar**: hanya memudarkan nada sebelum halftone (titik mengecil, tidak berlubang).
- Pada kaos berwarna, lubang juga membuang white underbase di titik itu (benar untuk DTF).

Pakai hanya tekstur yang Anda punya haknya. Kalau tekstur dibeli (mis. dari
TheVectorLab), lisensinya untuk bisnis Anda sendiri: jangan bagikan installer yang
berisi tekstur itu ke orang lain.
