"""Breakform_FidelityReport command — view the most recent import fidelity report."""
import FreeCAD
import FreeCADGui
from PySide2 import QtWidgets

try:
    from import_exl import _last_fidelity_report
except ImportError:
    _last_fidelity_report = None


class FidelityReportViewer:

    def GetResources(self):
        return {
            "MenuText": "Fidelity Report",
            "ToolTip": "View the fidelity report from the last import",
        }

    def IsActive(self):
        return _last_fidelity_report is not None

    def Activated(self):
        if _last_fidelity_report is None:
            QtWidgets.QMessageBox.information(
                None, "No Report",
                "No fidelity report available. Import an EXL file first.",
            )
            return

        dlg = QtWidgets.QDialog(None)
        dlg.setWindowTitle("Breakform — Fidelity Report")
        dlg.resize(640, 480)
        layout = QtWidgets.QVBoxLayout(dlg)

        text = QtWidgets.QPlainTextEdit()
        text.setReadOnly(True)
        text.setPlainText(_last_fidelity_report)
        layout.addWidget(text)

        close_btn = QtWidgets.QPushButton("Close")
        close_btn.clicked.connect(dlg.accept)
        layout.addWidget(close_btn)

        dlg.exec_()


FreeCADGui.addCommand("Breakform_FidelityReport", FidelityReportViewer())
