export const COMMANDS = {
  getStaticInfo: 'get_static_info',
  getSnapshot: 'get_snapshot',
  setDetailEnabled: 'set_detail_enabled',
  getSettings: 'get_settings',
  setSettings: 'set_settings',
  hidePopover: 'hide_popover',
  resizePopover: 'resize_popover',
  quitApp: 'quit_app',
  openLoginItemsSettings: 'open_login_items_settings',
} as const;

export const EVENTS = {
  snapshot: 'metrics://snapshot',
  disksChanged: 'disks://changed',
  popoverVisibility: 'popover://visibility',
} as const;

export const POPOVER_MIN_HEIGHT = 200;

export const POPOVER_MAX_HEIGHT = 640;
