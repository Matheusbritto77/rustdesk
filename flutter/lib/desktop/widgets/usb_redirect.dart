import 'dart:convert';

import 'package:flutter/material.dart';

import 'package:uuid/uuid.dart';

import '../../common.dart';
import '../../models/platform_model.dart';

class _UsbDevice {
  final String busId;
  final String name;
  final String deviceClass;
  final int vendorId;
  final int productId;
  bool selected;

  _UsbDevice.fromJson(Map<String, dynamic> json)
      : busId = json['bus_id'] as String? ?? '',
        name = json['name'] as String? ?? 'USB device',
        deviceClass = json['device_class'] as String? ?? 'USB',
        vendorId = json['vendor_id'] as int? ?? 0,
        productId = json['product_id'] as int? ?? 0,
        selected = json['is_selected'] as bool? ?? false;
}

/// Controller-side device selection. Attachment only happens when the
/// controlled host has a USB/IP virtual-host backend.
class UsbRedirectDialog extends StatefulWidget {
  /// The session through which attach/detach messages will be sent.
  final dynamic sessionId;
  const UsbRedirectDialog({Key? key, required this.sessionId}) : super(key: key);

  @override
  State<UsbRedirectDialog> createState() => _UsbRedirectDialogState();
}

class _UsbRedirectDialogState extends State<UsbRedirectDialog> {
  var _loading = true;
  var _backendStatus = '';
  var _devices = <_UsbDevice>[];

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    final raw = await bind.mainGetLocalUsbDevices();
    final decoded = jsonDecode(raw) as List<dynamic>;
    final backendStatus = await bind.mainGetUsbRedirectBackendStatus();
    if (!mounted) return;
    setState(() {
      _devices = decoded
          .whereType<Map<String, dynamic>>()
          .map(_UsbDevice.fromJson)
          .toList();
      _backendStatus = backendStatus;
      _loading = false;
    });
  }

  Future<void> _toggle(_UsbDevice device) async {
    final next = !device.selected;
    // Send the attach/detach request through the active remote session.
    final sentOk = bind.sessionToggleUsbRedirect(
      sessionId: widget.sessionId is UuidValue
          ? widget.sessionId as UuidValue
          : UuidValue(widget.sessionId.toString()),
      busId: device.busId,
      vendorId: device.vendorId,
      productId: device.productId,
      attach: next,
    );
    // Mirror the selection in local Rust state so the list stays in sync.
    if (sentOk) {
      bind.mainSetLocalUsbDeviceSelected(busId: device.busId, selected: next);
    }
    if (sentOk && mounted) setState(() => device.selected = next);
  }

  Future<void> _installBackend() async {
    setState(() => _loading = true);
    final result = await bind.mainInstallUsbRedirectBackend();
    if (!mounted) return;
    setState(() {
      _backendStatus = result;
      _loading = false;
    });
    await _load();
  }

  IconData _iconFor(_UsbDevice device) {
    final value = '${device.name} ${device.deviceClass}'.toLowerCase();
    if (value.contains('smart') ||
        value.contains('token') ||
        value.contains('card')) {
      return Icons.verified_user_outlined;
    }
    if (value.contains('print')) return Icons.print_outlined;
    return Icons.usb_rounded;
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return AlertDialog(
      title: Row(children: [
        const Icon(Icons.usb_rounded),
        const SizedBox(width: 10),
        Text(translate('USB redirection')),
      ]),
      content: SizedBox(
        width: 560,
        child: Column(mainAxisSize: MainAxisSize.min, children: [
          Container(
            width: double.infinity,
            padding: const EdgeInsets.all(12),
            color: theme.colorScheme.surfaceContainerHighest,
            child: Text(translate(_backendStatus),
                style: theme.textTheme.bodySmall),
          ),
          const SizedBox(height: 12),
          if (_loading)
            const Padding(
                padding: EdgeInsets.all(24), child: CircularProgressIndicator())
          else if (_devices.isEmpty)
            Padding(
              padding: const EdgeInsets.all(24),
              child: Text(translate('No USB devices found')),
            )
          else
            Flexible(
              child: ListView.separated(
                shrinkWrap: true,
                itemCount: _devices.length,
                separatorBuilder: (_, __) => const Divider(height: 1),
                itemBuilder: (_, index) {
                  final device = _devices[index];
                  final ids =
                      'VID:${device.vendorId.toRadixString(16).padLeft(4, '0').toUpperCase()} PID:${device.productId.toRadixString(16).padLeft(4, '0').toUpperCase()}';
                  return ListTile(
                    leading: Icon(_iconFor(device)),
                    title: Text(device.name,
                        maxLines: 1, overflow: TextOverflow.ellipsis),
                    subtitle: Text('${device.deviceClass}  |  $ids',
                        maxLines: 1, overflow: TextOverflow.ellipsis),
                    trailing: Switch(
                        value: device.selected,
                        onChanged: (_) => _toggle(device)),
                  );
                },
              ),
            ),
        ]),
      ),
      actions: [
        TextButton(
            onPressed: _installBackend,
            child: Text(translate('Activate USB/IP'))),
        TextButton(onPressed: _load, child: Text(translate('Refresh'))),
        TextButton(
            onPressed: () => Navigator.of(context).pop(),
            child: Text(translate('Close'))),
      ],
    );
  }
}
