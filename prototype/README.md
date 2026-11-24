# Prototipe engine (acuan algoritma)

Implementasi Python dari engine DTF StudioZea. Ini **spesifikasi** untuk engine Rust:
semua algoritma di sini sudah diuji di [`docs/RENCANA-DTF.md`](../docs/RENCANA-DTF.md)
(§3, §4, §7, §9b). Engine Rust harus menghasilkan piksel yang sama untuk setting yang
sama. Ini bukan aplikasinya, dan tidak dioptimalkan.

| File | Isi |
|---|---|
| `studiozea_ref.py` | engine: screen rasional ber-ranking, hybrid AM/FM per bentuk, kalibrasi otomatis, pembersihan DTF, mode kaos hitam/putih/berwarna, channel K, tepi tajam, hapus background (matting lokal), metrik |
| `render.py` | command line: gambar → PNG siap cetak |
| `uji_lpi_bentuk.py` | uji semua bentuk × LPI × kaos (color chart 60 warna + gradasi), mengulang tabel §9b |

## Menjalankan

```bash
pip install -r requirements.txt

# kaos hitam, 35 LPI, lebar 30 cm, 300 DPI, auto-trim
python render.py desain.png hasil.png --kaos hitam --lpi 35 --lebar-cm 30 --trim

# kaos putih, background berwarna dihapus, bentuk elips 45 LPI, channel K
python render.py desain.png hasil.png --kaos putih --hapus-bg warna --bentuk elips --lpi 45 --k gcr

# kaos berwarna (kode warna kaos), pola grain
python render.py desain.png hasil.png --kaos "#1C2541" --pola grain-sedang

# uji lengkap (~8 menit)
python uji_lpi_bentuk.py
```

Opsi: `--bentuk bulat|bulat-murni|elips|kotak|berlian|garis|silang`,
`--pola am|fm|grain-halus|grain-sedang|grain-kasar`, `--grid auto|rapi|presisi`,
`--dpi`, `--k off|garis|gcr`, `--tepi-tajam`, `--knockout 0.08` (kaos berwarna,
hemat tinta).

Setiap file yang ditulis selalu dicek dulu: tidak ada piksel semi-transparan,
tidak ada warna yang tidak murni, dan tidak ada warna di bawah piksel transparan.
Kalau ada yang gagal, file tidak disimpan.

## Hasil yang sudah diverifikasi
- Identik piksel per piksel dengan hasil uji v5 untuk RTH (kaos hitam),
  Powerslave (kaos putih, CMY dan GCR), PNG transparan → kaos merah, dan RTH → kaos
  navy.
- `uji_lpi_bentuk.py` memberi angka yang sama dengan sweep v5 (contoh: kotak 35 LPI
  kaos hitam ΔE 0,27; bulat 65 LPI kaos putih ΔE 0,39).

Gambar referensi/desain klien **tidak** disimpan di repo (hak cipta). Simpan di
`refs/` (sudah di-ignore).
