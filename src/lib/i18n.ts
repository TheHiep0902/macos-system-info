export const messages = {
  'app.name': 'Pulsebar',
  'common.loading': 'Đang tải…',
  'common.placeholder': '—',
  'common.back': 'Quay lại',
  'status.normal': 'Bình thường',
  'status.warn': 'Cao',
  'status.critical': 'Rất cao',
  'status.unavailable': 'Không khả dụng',
  'unit.day': '{count} ngày',
  'unit.hour': '{count} giờ',
  'unit.minute': '{count} phút',
  'notice.data': 'Không lấy được số liệu từ Pulsebar.',
  'notice.action': 'Không thực hiện được thao tác. Hãy thử lại.',
  'header.platform.appleSilicon': 'Apple Silicon',
  'header.platform.intel': 'Intel',
  'header.rosetta': 'Rosetta',
  'header.rosettaHint': 'Đang chạy qua Rosetta, một số cảm biến không khả dụng',
  'header.uptime': 'Hoạt động {uptime}',
  'header.starting': 'Đang khởi động bộ đo…',
  'cpu.title': 'CPU',
  'cpu.openDetail': 'Chi tiết',
  'cpu.openDetailLabel': 'Xem chi tiết CPU',
  'cpu.usageLabel': 'Mức sử dụng CPU',
  'cpu.average': 'Trung bình {mhz}',
  'cpu.nominal': 'Cơ bản {mhz}',
  'cpu.cluster': '{label}: {mhz}',
  'cpu.frequencyLoading': 'Đang đo xung nhịp…',
  'chart.summary': 'Hai phút gần nhất: thấp nhất {min}, cao nhất {max}',
  'chart.collecting': 'Đang thu thập lịch sử',
  'power.title': 'Công suất',
  'power.scope.cpu': 'CPU',
  'power.scope.package': 'Gói CPU',
  'power.scopeLabel': 'Phạm vi: {scope}',
  'power.loading': 'Đang đo công suất…',
  'memory.title': 'Bộ nhớ',
  'memory.usage': '{used} / {total}',
  'memory.usageLabel': 'Mức sử dụng bộ nhớ',
  'memory.app': 'Ứng dụng',
  'memory.wired': 'Cố định',
  'memory.compressed': 'Đã nén',
  'memory.cached': 'Bộ nhớ đệm',
  'memory.swap': 'Swap đã dùng',
  'disk.title': 'Ổ đĩa',
  'disk.boot': 'Khởi động',
  'disk.removable': 'Ổ rời',
  'disk.network': 'Ổ mạng',
  'disk.usage': '{used} / {total}',
  'disk.free': 'Còn trống {size}',
  'disk.available': 'Khả dụng {size}',
  'disk.purgeableNote': 'Khả dụng gồm dung lượng purgeable',
  'disk.usageLabel': 'Mức sử dụng {name}',
  'disk.empty': 'Không có ổ đĩa nào',
  'disk.updating': 'Đang cập nhật danh sách ổ đĩa…',
  'network.title': 'Wi-Fi',
  'network.interface': 'Giao diện',
  'network.ipv4': 'IPv4',
  'network.ipv6': 'IPv6',
  'network.noAddress': 'Chưa có địa chỉ IP',
  'network.disconnected': 'Không kết nối Wi-Fi',
  'network.primary': 'Đang dùng mạng qua {iface}',
  'uptime.title': 'Thời gian hoạt động',
  'uptime.bootedAt': 'Khởi động lúc',
  'uptime.os': 'macOS',
  'uptime.model': 'Model',
  'footer.settings': 'Cài đặt',
  'footer.quit': 'Thoát Pulsebar',
  'detail.title': 'Chi tiết CPU',
  'detail.topology': '{physical} lõi vật lý / {logical} luồng',
  'detail.topologyAssumed': 'Cách chia cụm lõi đang được suy đoán, có thể chưa chính xác.',
  'detail.perCoreUnsupported': 'Không hỗ trợ MHz từng lõi trên Mac này',
  'detail.coresLoading': 'Đang lấy số liệu từng lõi…',
  'detail.coresEmpty': 'Chưa có số liệu từng lõi',
  'detail.clusters': 'Cụm lõi',
  'detail.cores': 'Từng lõi',
  'detail.threads': 'Từng CPU logic',
  'detail.coreLabel': 'Lõi {id}',
  'detail.cpuLabel': 'CPU {id}',
  'detail.coreUsageLabel': 'Mức sử dụng {name}',
  'detail.groupHeading': '{name} · {count}',
  'cluster.E': 'Lõi tiết kiệm (E)',
  'cluster.P': 'Lõi hiệu năng (P)',
  'cluster.S': 'Lõi siêu hiệu năng (S)',
  'cluster.CPU': 'CPU',
  'cluster.max': 'Tối đa {mhz}',
  'cluster.usage': 'Sử dụng',
  'cluster.frequency': 'Xung nhịp',
  'freq.title': 'Xung nhịp',
  'freq.avg': 'Trung bình',
  'freq.min': 'Thấp nhất',
  'freq.max': 'Cao nhất',
  'freq.speedLimit': 'Giới hạn tốc độ',
  'freq.nominal': 'Cơ bản {nominal} · Tối đa {max} · Giới hạn tốc độ {pct}',
  'freq.loading': 'Đang đo xung nhịp…',
  'source.ioReport': 'IOReport',
  'source.nominal': 'Danh định',
  'source.smc': 'SMC · {key}',
  'source.label': 'Nguồn số liệu: {source}',
  'settings.title': 'Cài đặt',
  'settings.menuBar': 'Thanh menu',
  'settings.titleFormat': 'Kiểu tiêu đề',
  'settings.titleFormat.default': 'Mặc định',
  'settings.titleFormat.compact': 'Thu gọn',
  'settings.showTrayIcon': 'Hiện biểu tượng',
  'settings.appearance': 'Giao diện',
  'settings.theme': 'Chủ đề',
  'settings.theme.system': 'Theo hệ thống',
  'settings.theme.light': 'Sáng',
  'settings.theme.dark': 'Tối',
  'settings.reduceEffects': 'Giảm hiệu ứng trong suốt',
  'settings.disks': 'Ổ đĩa',
  'settings.showNetworkDisks': 'Hiện ổ mạng',
  'settings.system': 'Hệ thống',
  'settings.memorySaver': 'Tiết kiệm bộ nhớ',
  'settings.memorySaverHint': 'Giải phóng cửa sổ sau 5 phút ẩn',
  'settings.launchAtLogin': 'Khởi động cùng macOS',
  'settings.quit': 'Thoát Pulsebar',
  'settings.saving': 'Đang lưu…',
  'settings.saveError': 'Không lưu được cài đặt, đã khôi phục giá trị trước.',
  'settings.loadError': 'Không tải được cài đặt.',
} as const;

type Messages = typeof messages;

export type MessageKey = keyof Messages;

type Placeholders<S extends string> = S extends `${string}{${infer Name}}${infer Rest}`
  ? Name | Placeholders<Rest>
  : never;

export type MessageParams<K extends MessageKey> = Record<
  Placeholders<Messages[K]>,
  string | number
>;

type ParamArgs<K extends MessageKey> = [Placeholders<Messages[K]>] extends [never]
  ? []
  : [params: MessageParams<K>];

const PLACEHOLDER_PATTERN = /\{(\w+)\}/g;

export function t<K extends MessageKey>(key: K, ...args: ParamArgs<K>): string {
  const template: string = messages[key];
  const params: Record<string, string | number> | undefined = args[0];
  if (params === undefined) {
    return template;
  }
  return template.replace(PLACEHOLDER_PATTERN, (match, name: string) => {
    const value = params[name];
    return value === undefined ? match : String(value);
  });
}
