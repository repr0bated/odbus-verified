map(select(.phase=="cycle_end")) as $c |
($c|length) as $n |
($c|map(.cycle)|add / $n) as $mx |
($c|map(.rust_live_bytes)|add / $n) as $my |
([$c[]|(.cycle-$mx)*(.rust_live_bytes-$my)]|add) as $xy |
([$c[]|(.cycle-$mx)*(.cycle-$mx)]|add) as $xx |
[ $filename, $n, $c[0].rust_live_bytes, $c[-1].rust_live_bytes,
  ($c[-1].rust_live_bytes-$c[0].rust_live_bytes), ($xy/$xx),
  $c[0].rss_kib, $c[-1].rss_kib, (map(.rss_kib//0)|max),
  $c[0].anonymous_kib, $c[-1].anonymous_kib,
  ($c|map(.fds)|min),($c|map(.fds)|max),($c|map(.threads)|min),($c|map(.threads)|max),
  ($c|map(.children)|max),
  (($c[-1].wall_ns-$c[0].wall_ns)/1000000000), (($c[-1].cpu_ns-$c[0].cpu_ns)/1000000000),
  ($c|map(.latency_p50_ns)|min),($c|map(.latency_p50_ns)|max),($c|map(.latency_p95_ns)|max),
  (map(select(.phase=="final_clear" or .phase=="released"))|last|.rust_live_bytes//0),
  (map(select(.phase=="cache_dropped" or .phase=="released"))|last|.rust_live_bytes//0)
] | @tsv
