# Snapshot status IDX untuk paper trading

Guardrail membaca snapshot **manual**, bukan feed real-time. Verifikasi ticker terhadap [pengumuman BEI](https://www.idx.co.id/id/berita/pengumuman/), [pengumuman UMA](https://www.idx.co.id/id/berita/unusual-market-activity-uma), serta pengumuman suspensi/pencabutan dan daftar Papan Pemantauan Khusus yang berlaku. Jangan menandai `false` jika statusnya belum dikonfirmasi.

Simpan hasil verifikasi di `docker-data/idx/market-status.json` di akar proyek. Format contoh berikut menggunakan ticker fiktif; jangan dipakai sebagai data pasar:

```json
{
  "verified_at": "2026-11-26T10:00:00+07:00",
  "symbols": [
    {"ticker": "CONTOH", "fca": false, "uma": true, "suspended": false}
  ]
}
```

- `verified_at` adalah waktu pengecekan **seluruh** snapshot. Gunakan ISO 8601 dengan zona waktu (`+07:00` untuk WIB). Jangan memperbarui timestamp tanpa memeriksa ulang.
- Cantumkan hanya saham yang ketiga statusnya diketahui. Flag `true` memblokir entry. Ketiganya harus `false` agar cek event-risk IDX lolos; guardrail lain tetap berlaku. Ticker yang tidak ada, snapshot kedaluwarsa (usia 24 jam atau lebih), timestamp di masa depan, dan file yang tidak valid memblokir entry.
- Simpan pembaruan sebagai satu file utuh. Docker memasang folder lokal itu sebagai *read-only*; perubahan file dibaca pada permintaan berikutnya tanpa rebuild. Untuk proses Rust di luar Docker, `IDX_STATUS_SNAPSHOT_PATH` bisa menunjuk lokasi lain.
- Folder `docker-data/` diabaikan Git. Perbarui snapshot setelah ada pengumuman baru; batas 24 jam bukan jaminan bahwa tidak ada perubahan antara dua pengecekan.
