// Fish Dock hosts zashboard in a dock tab, and the kernel's controller port
// falls back to a random one whenever 9099 is busy — so the panel's
// localStorage origin, and every saved preference with it, changes between
// starts. zashboard's stock default for config/is-sidebar-collapsed is true,
// which leaves a bare icon rail without the traffic chart, stats, backend
// switcher and core version. Re-assert the expanded sidebar on every load so
// the panel always opens the way the dock expects.
try {
  localStorage.setItem('config/is-sidebar-collapsed', 'false')
} catch (error) {}
