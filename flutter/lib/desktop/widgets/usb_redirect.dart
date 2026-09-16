import 'package:flutter/material.dart';
import 'package:get/get.dart';
import '../../common.dart';

class UsbDeviceItem {
  final String busId;
  final String name;
  final String deviceClass;
  final String vendorProduct;
  final IconData icon;
  bool isRedirected;

  UsbDeviceItem({
    required this.busId,
    required this.name,
    required this.deviceClass,
    required this.vendorProduct,
    required this.icon,
    this.isRedirected = false,
  });
}

class UsbRedirectDialog extends StatefulWidget {
  const UsbRedirectDialog({Key? key}) : super(key: key);

  @override
  State<UsbRedirectDialog> createState() => _UsbRedirectDialogState();
}

class _UsbRedirectDialogState extends State<UsbRedirectDialog> {
  bool _autoRedirectTokens = true;

  final List<UsbDeviceItem> _devices = [
    UsbDeviceItem(
      busId: '1-1.2',
      name: 'SafeNet eToken 5110 (Token A3)',
      deviceClass: 'SmartCard / Certificado',
      vendorProduct: 'Vendor: 0529 | Product: 0620',
      icon: Icons.shield_rounded,
      isRedirected: true,
    ),
    UsbDeviceItem(
      busId: '1-1.4',
      name: 'Kingston DataTraveler 3.0 (32GB)',
      deviceClass: 'Armazenamento USB',
      vendorProduct: 'Vendor: 0951 | Product: 1666',
      icon: Icons.sd_storage_rounded,
      isRedirected: false,
    ),
    UsbDeviceItem(
      busId: '2-1.1',
      name: 'HP LaserJet M1132 MFP',
      deviceClass: 'Impressora USB',
      vendorProduct: 'Vendor: 03F0 | Product: 042A',
      icon: Icons.print_rounded,
      isRedirected: false,
    ),
  ];

  void _toggleDevice(UsbDeviceItem item) {
    setState(() {
      item.isRedirected = !item.isRedirected;
    });
  }

  @override
  Widget build(BuildContext context) {
    return Dialog(
      backgroundColor: Colors.transparent,
      child: Container(
        width: 540,
        padding: const EdgeInsets.all(20),
        decoration: BoxDecoration(
          color: const Color(0xFF1E1E2E).withOpacity(0.95),
          borderRadius: BorderRadius.circular(16),
          border: Border.all(color: Colors.white24, width: 1.5),
          boxShadow: [
            BoxShadow(
              color: Colors.black.withOpacity(0.5),
              blurRadius: 20,
              spreadRadius: 2,
            ),
          ],
        ),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            // Header
            Row(
              children: [
                Container(
                  padding: const EdgeInsets.all(8),
                  decoration: BoxDecoration(
                    color: MyTheme.accent.withOpacity(0.2),
                    borderRadius: BorderRadius.circular(10),
                  ),
                  child: const Icon(
                    Icons.usb_rounded,
                    color: MyTheme.accent,
                    size: 24,
                  ),
                ),
                const SizedBox(width: 12),
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        translate('Dispositivos USB (USB over IP)'),
                        style: const TextStyle(
                          color: Colors.white,
                          fontSize: 18,
                          fontWeight: FontWeight.bold,
                        ),
                      ),
                      Text(
                        translate('Redirecione tokens, impressoras e pendrives para o computador remoto'),
                        style: const TextStyle(
                          color: Colors.white60,
                          fontSize: 12,
                        ),
                      ),
                    ],
                  ),
                ),
                IconButton(
                  onPressed: () => Navigator.of(context).pop(),
                  icon: const Icon(Icons.close, color: Colors.white70),
                ),
              ],
            ),
            const Divider(color: Colors.white12, height: 24),

            // Devices List
            Flexible(
              child: SingleChildScrollView(
                child: Column(
                  children: _devices.map((device) {
                    return Container(
                      margin: const EdgeInsets.only(bottom: 10),
                      padding: const EdgeInsets.all(12),
                      decoration: BoxDecoration(
                        color: device.isRedirected
                            ? MyTheme.accent.withOpacity(0.12)
                            : Colors.white.withOpacity(0.04),
                        borderRadius: BorderRadius.circular(12),
                        border: Border.all(
                          color: device.isRedirected
                              ? MyTheme.accent.withOpacity(0.5)
                              : Colors.white10,
                          width: 1.5,
                        ),
                      ),
                      child: Row(
                        children: [
                          Icon(
                            device.icon,
                            color: device.isRedirected
                                ? MyTheme.accent
                                : Colors.white70,
                            size: 28,
                          ),
                          const SizedBox(width: 14),
                          Expanded(
                            child: Column(
                              crossAxisAlignment: CrossAxisAlignment.start,
                              children: [
                                Text(
                                  device.name,
                                  style: const TextStyle(
                                    color: Colors.white,
                                    fontWeight: FontWeight.bold,
                                    fontSize: 14,
                                  ),
                                ),
                                const SizedBox(height: 2),
                                Text(
                                  device.vendorProduct,
                                  style: const TextStyle(
                                    color: Colors.white54,
                                    fontSize: 11,
                                    fontFamily: 'monospace',
                                  ),
                                ),
                                const SizedBox(height: 4),
                                Row(
                                  children: [
                                    Container(
                                      width: 8,
                                      height: 8,
                                      decoration: BoxDecoration(
                                        shape: BoxShape.circle,
                                        color: device.isRedirected
                                            ? Colors.greenAccent
                                            : Colors.grey,
                                      ),
                                    ),
                                    const SizedBox(width: 6),
                                    Text(
                                      device.isRedirected
                                          ? translate('● Em Uso na Sessão Remota')
                                          : translate('○ Disponível Localmente'),
                                      style: TextStyle(
                                        color: device.isRedirected
                                            ? Colors.greenAccent
                                            : Colors.white54,
                                        fontSize: 11,
                                        fontWeight: FontWeight.w500,
                                      ),
                                    ),
                                  ],
                                ),
                              ],
                            ),
                          ),
                          const SizedBox(width: 10),
                          InkWell(
                            onTap: () => _toggleDevice(device),
                            borderRadius: BorderRadius.circular(8),
                            child: Container(
                              padding: const EdgeInsets.symmetric(
                                  horizontal: 12, vertical: 8),
                              decoration: BoxDecoration(
                                color: device.isRedirected
                                    ? Colors.redAccent.withOpacity(0.2)
                                    : MyTheme.accent,
                                borderRadius: BorderRadius.circular(8),
                                border: Border.all(
                                  color: device.isRedirected
                                      ? Colors.redAccent
                                      : MyTheme.accent,
                                  width: 1,
                                ),
                              ),
                              child: Text(
                                device.isRedirected
                                    ? translate('DESCONECTAR')
                                    : translate('REDIRECIONAR'),
                                style: TextStyle(
                                  color: device.isRedirected
                                      ? Colors.redAccent
                                      : Colors.white,
                                  fontWeight: FontWeight.bold,
                                  fontSize: 11,
                                ),
                              ),
                            ),
                          ),
                        ],
                      ),
                    );
                  }).toList(),
                ),
              ),
            ),
            const Divider(color: Colors.white12, height: 24),

            // Footer Options
            Row(
              children: [
                Checkbox(
                  value: _autoRedirectTokens,
                  activeColor: MyTheme.accent,
                  onChanged: (val) {
                    setState(() {
                      _autoRedirectTokens = val ?? false;
                    });
                  },
                ),
                Expanded(
                  child: Text(
                    translate('Redirecionar automaticamente Tokens A3 / Certificados ao conectar'),
                    style: const TextStyle(color: Colors.white70, fontSize: 12),
                  ),
                ),
              ],
            ),
          ],
        ),
      ),
    );
  }
}
