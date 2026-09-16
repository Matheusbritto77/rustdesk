import 'package:auto_size_text/auto_size_text.dart';
import 'package:flutter/material.dart';
import 'package:get/get.dart';

import '../../common.dart';

Widget getConnectionPageTitle(BuildContext context, bool isWeb) {
  return Row(
    children: [
      Expanded(
        child: Row(
          children: [
            Flexible(
              child: AutoSizeText(
                translate('Control Remote Desktop'),
                maxLines: 1,
                minFontSize: 12,
                overflow: TextOverflow.ellipsis,
                style: Theme.of(context)
                    .textTheme
                    .titleLarge
                    ?.merge(const TextStyle(height: 1)),
              ).marginOnly(right: 6),
            ),
            Tooltip(
              waitDuration: const Duration(milliseconds: 300),
              message: translate(isWeb ? "web_id_input_tip" : "id_input_tip"),
              child: Icon(
                Icons.help_outline_outlined,
                size: 16,
                color: Theme.of(context)
                    .textTheme
                    .titleLarge
                    ?.color
                    ?.withOpacity(0.5),
              ),
            ),
          ],
        ),
      ),
    ],
  );
}
