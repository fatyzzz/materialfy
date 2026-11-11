// Островок на macOS: окно над строкой меню (её уровень — NSMainMenuWindowLevel, обычное «поверх всех» Qt —
// ниже), на всех рабочих столах, но не над полноэкранными приложениями (у них свой рабочий стол), не прячется,
// когда приложение неактивно (Qt::Tool — NSPanel с hidesOnDeactivate). Размер выреза экрана — в свойства окна
// notchW / notchH (FloatingIsland.qml), без выреза — 0: островок тогда как на Windows, полоской у кромки.
#include "floating.h"

#import <AppKit/AppKit.h>

#include <QtGui/QGuiApplication>
#include <QtGui/QWindow>

#include <cmath>

namespace mf {

bool
floating_tool_window(QObject* object)
{
  auto* window = qobject_cast<QWindow*>(object);
  if (!window || QGuiApplication::platformName() != QLatin1String("cocoa"))
    return false;
  NSView* view = reinterpret_cast<NSView*>(window->winId());
  NSWindow* ns = view.window;
  if (!ns)
    return false;
  ns.level = NSMainMenuWindowLevel + 1;
  ns.collectionBehavior = NSWindowCollectionBehaviorCanJoinAllSpaces | NSWindowCollectionBehaviorStationary
                          | NSWindowCollectionBehaviorIgnoresCycle;
  ns.hidesOnDeactivate = NO;
  ns.hasShadow = NO;

  int notch_w = 0;
  int notch_h = 0;
  if (@available(macOS 12.0, *)) {
    NSScreen* screen = ns.screen;
    if (screen && screen.safeAreaInsets.top > 0) {
      // вырез — между двумя видимыми кусками строки меню слева и справа от него
      const NSRect l = screen.auxiliaryTopLeftArea;
      const NSRect r = screen.auxiliaryTopRightArea;
      notch_h = static_cast<int>(std::lround(screen.safeAreaInsets.top));
      notch_w = static_cast<int>(std::lround(NSWidth(screen.frame) - NSWidth(l) - NSWidth(r)));
    }
  }
  window->setProperty("notchW", notch_w);
  window->setProperty("notchH", notch_h);
  return true;
}

} // namespace mf
