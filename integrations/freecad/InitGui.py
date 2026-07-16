"""GUI initialization for Breakform workbench."""
import FreeCADGui


class BreakformWorkbench:
    """Breakform workbench: honest engineering-data interchange"""

    MenuText = "Breakform"
    ToolTip = "Break the format. Keep the truth."
    Icon = ""

    def Initialize(self):
        commands = [
            "Breakform_ImportEXL",
            "Breakform_ExportEXL",
            "Breakform_FidelityReport",
        ]
        self.appendToolbar("Breakform", commands)
        self.appendMenu("Breakform", commands)

    def Activated(self):
        pass

    def Deactivated(self):
        pass


FreeCADGui.addWorkbench(BreakformWorkbench())
