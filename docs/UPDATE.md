# Update otomatis StudioZea

Mulai versi 0.2.0 aplikasi mengecek sendiri apakah ada versi baru (3 detik setelah dibuka,
dan lewat tombol **cek update** (ikon ⟳) di kanan atas). Kalau ada, muncul bar
**"Versi x.y.z tersedia … [Update sekarang] [Nanti]"**. Klik *Update sekarang*: file diunduh
(dengan progres), tanda tangannya dicek, installer dipasang (mode pasif) dan aplikasi terbuka
lagi. Tidak ada yang terpasang tanpa klik. Pengaturan, preset dan proyek tidak hilang.

## Cara kerjanya

- Repo kode `yopish04/studiozea` tetap **privat**. File untuk publik (installer + `latest.json`)
  ada di repo **publik** `yopish04/studiozea-update`, hanya berisi release.
- Aplikasi membaca
  `https://github.com/yopish04/studiozea-update/releases/latest/download/latest.json`
  (lihat `app/src-tauri/tauri.conf.json` → `plugins.updater`).
- Setiap update **ditandatangani** dengan kunci privat. Kunci publiknya tertanam di aplikasi
  (`pubkey`); file yang tanda tangannya tidak cocok ditolak ("file update tidak sah").

## Di mana build & rilis berjalan

Kode ada di repo **publik** `yopish04/goblon`, jadi menit GitHub Actions gratis tanpa batas
(Windows juga). Workflow: `.github/workflows/studiozea-release.yml`. Installer + `latest.json`
untuk publik ada di `yopish04/studiozea-update`.

Setiap push (semua branch), pull request atau *Run workflow*: tes engine (Linux + Windows) dan
build uji Windows. Installer ada di bagian *Artifacts* halaman run itu (14 hari).

## Persiapan rilis (sekali saja)

Tanpa secret di bawah ini hanya build uji yang dibuat. Untuk rilis + update otomatis, di repo
goblon: *Settings → Secrets and variables → Actions → New repository secret*:
- `TAURI_SIGNING_PRIVATE_KEY` = isi file `studiozea-update.key`. Tempel **apa adanya**; spasi /
  baris baru dibuang otomatis (sebelumnya baris baru di secret membuat build gagal:
  *failed to decode base64 key: Invalid symbol 10*).
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` hanya kalau kuncinya pakai password.
- `UPDATE_REPO_TOKEN` = token fine-grained (GitHub → *Settings → Developer settings →
  Personal access tokens → Fine-grained tokens*), hanya repo `studiozea-update`,
  **Contents: Read and write**, masa berlaku selama mungkin.

Simpan file kunci di tempat aman. **Kalau kunci hilang, aplikasi yang sudah terpasang tidak bisa
menerima update lagi.** Jangan pernah commit kunci ini (repo ini publik).

## Merilis versi baru (otomatis)

1. Naikkan `version` di `app/src-tauri/tauri.conf.json` (mis. 0.2.0 → 0.2.1), juga
   `app/package.json` dan `app/src-tauri/Cargo.toml` supaya sama.
2. Tulis catatan rilis di `RILIS.md` (baris pertama tampil di bar update).
3. Push ke `main`. Kalau versi itu belum ada di `studiozea-update` (dan secret sudah ada),
   installer bertanda tangan langsung dibangun dan diterbitkan. Ada juga pengecekan harian
   (08:23 WIB) sebagai cadangan.

Versi harus selalu lebih tinggi dari sebelumnya. Jadwal harian dimatikan GitHub kalau repo
tidak ada aktivitas 60 hari; aktifkan lagi di tab Actions bila perlu.

## Catatan

- Versi 0.1.0 belum punya fitur ini: pasang 0.2.0 sekali secara manual; setelah itu update
  cukup dengan klik.
- Peringatan Windows SmartScreen ("Windows protected your PC") saat install pertama terjadi
  karena installer belum memakai sertifikat code-signing Windows (berbayar). Itu terpisah
  dari tanda tangan update di atas.
