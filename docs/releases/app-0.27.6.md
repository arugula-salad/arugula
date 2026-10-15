# Arugula app 0.27.6

- **Blocks stay in the window on macOS and Linux** (#929, #150): a port or
  editor block opened from control's page went to the browser instead of
  its frame, because the window's navigation check also saw the frame's.
  Blocks under control's block domain (and the daemon's own `localhost`
  block sites) now load in place. Other outside frames still open in the
  browser (#930).

It carries arugulad 0.39.3.
