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
  getUpdateState: 'get_update_state',
  checkForUpdates: 'check_for_updates',
  installUpdate: 'install_update',
} as const;

export const EVENTS = {
  snapshot: 'metrics://snapshot',
  disksChanged: 'disks://changed',
  popoverVisibility: 'popover://visibility',
  updaterState: 'updater://state',
} as const;

export const POPOVER_MIN_HEIGHT = 200;

export const POPOVER_MAX_HEIGHT = 640;
