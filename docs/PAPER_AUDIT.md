# Paper Trading Audit API

`GET /api/paper/audit` hanya membaca data. `window` berisi 30 hari terakhir (`start` dan `end` dalam UTC RFC 3339). Tidak ada parameter, migrasi, atau perubahan pada order.

| Bagian | Isi |
|---|---|
| `entry_drift` | `sample_count` dan `mean_absolute_pct` dari `entry_drift_pct` pada sinyal `BUY`/`SELL` dengan nilai tersedia. Ini deviasi dari harga referensi saat keputusan, bukan slippage fill. |
| `guardrail` | Jumlah sinyal directional, jumlah yang ditolak, dan `rejection_reasons` berupa peta kode guardrail ke jumlah sinyal yang terkena. Satu sinyal dapat terkena lebih dari satu kode. |
| `closed_orders` | `sample_count` untuk order berstatus `closed` dengan `closed_at` dalam jendela, dan `realized_sample_count` untuk yang memiliki P&L terealisasi. |
| `closed_orders.r_multiple` | Distribusi dan rata-rata `realized_pnl / (jarak entry ke stop × size)`, memakai harga fill jika ada atau harga entry tercatat jika tidak. Sampel tanpa risiko awal yang valid dikeluarkan. Batas distribusi: `<0`, `=0`, `(0,1)`, `[1,2)`, `≥2`. |
| `closed_orders.confidence_calibration` | Win rate (`realized_pnl > 0`) dan jumlah sampel per bucket confidence 20 poin, dari order yang terhubung ke sinyal atau scout pick. Batas atas bucket eksklusif, kecuali 100%. |

Angka desimal dikirim sebagai string. Jika metrik tidak punya sampel, nilainya `null`. Audit tidak menjumlahkan P&L nominal antar mata uang.
