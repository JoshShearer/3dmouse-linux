# 3DMouse Linux configuration GUI (SRS §4.1, FR-800).
# Builds against Qt >= 6.2 (Ubuntu 22.04) up to current Qt 6.
cmake_minimum_required(VERSION 3.16)
project(3dmouse-config VERSION 0.1.0 LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_AUTOMOC ON)
set(CMAKE_AUTORCC ON)

find_package(Qt6 6.2 REQUIRED COMPONENTS Core Gui Network Qml Quick QuickControls2)

add_executable(3dmouse-config
    src/main.cpp
    src/agentclient.cpp
    src/agentclient.h
    qml.qrc
)
target_compile_definitions(3dmouse-config PRIVATE SM_VERSION="${PROJECT_VERSION}")
target_link_libraries(3dmouse-config PRIVATE Qt6::Core Qt6::Gui Qt6::Network Qt6::Qml Qt6::Quick Qt6::QuickControls2)

# On-screen overlay. LayerShellQt (wlr-layer-shell) is optional: without it the
# overlay only works on X11 and the agent keeps using notifications on Wayland.
find_package(LayerShellQt QUIET)
add_executable(3dmouse-osd
    src/osd_main.cpp
    src/agentclient.cpp
    src/agentclient.h
    osd.qrc
)
target_compile_definitions(3dmouse-osd PRIVATE SM_VERSION="${PROJECT_VERSION}")
target_link_libraries(3dmouse-osd PRIVATE Qt6::Core Qt6::Gui Qt6::Network Qt6::Qml Qt6::Quick)
if(LayerShellQt_FOUND)
    target_compile_definitions(3dmouse-osd PRIVATE SM_HAVE_LAYERSHELL=1)
    target_link_libraries(3dmouse-osd PRIVATE LayerShellQt::Interface)
    message(STATUS "3dmouse-osd: LayerShellQt ${LayerShellQt_VERSION} found, Wayland overlays enabled")
else()
    message(STATUS "3dmouse-osd: LayerShellQt not found, overlays on X11 only")
endif()

# Optional status indicator (FR-807): a StatusNotifierItem through QSystemTrayIcon.
find_package(Qt6 6.2 QUIET COMPONENTS Widgets)
if(Qt6Widgets_FOUND)
    add_executable(3dmouse-tray
        src/tray_main.cpp
        src/agentclient.cpp
        src/agentclient.h
        tray.qrc
    )
    target_compile_definitions(3dmouse-tray PRIVATE SM_VERSION="${PROJECT_VERSION}")
    target_link_libraries(3dmouse-tray PRIVATE Qt6::Core Qt6::Gui Qt6::Network Qt6::Qml Qt6::Widgets)
    set(SM_TRAY_TARGET 3dmouse-tray)
else()
    message(STATUS "3dmouse-tray: Qt6 Widgets not found, status indicator not built")
endif()

include(GNUInstallDirs)
install(TARGETS 3dmouse-config 3dmouse-osd ${SM_TRAY_TARGET} RUNTIME DESTINATION ${CMAKE_INSTALL_BINDIR})
install(FILES assets/io.github.joshshearer._3dmouse_linux.config.desktop DESTINATION ${CMAKE_INSTALL_DATADIR}/applications)
install(FILES assets/3dmouse-linux.svg DESTINATION ${CMAKE_INSTALL_DATADIR}/icons/hicolor/scalable/apps)
