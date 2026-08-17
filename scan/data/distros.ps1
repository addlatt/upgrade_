# =============================================================================
#  upgrade_ / distribution kernel table
# =============================================================================
#  This table goes stale faster than anything else in the project. The scanner
#  prints a warning when it is older than 120 days.
#
#  MAINTAINERS: verify these against the distro's own release notes before
#  each release. Numbers marked Approx=$true were not verified against a
#  primary source and should be confirmed or corrected.
#
#  Kernel      the kernel a fresh install of the current stable release gives
#              you on first boot. NOT the newest kernel available in backports.
#  Policy      how the kernel moves over the life of the release.
#  NvidiaEasy  does the installer offer the proprietary NVIDIA driver in a way
#              a first-time user will actually find?
# =============================================================================

$script:UpgDistroTableVerified = '2026-08-16'

function Get-UpgDistroTable {
    @(
        @{ Name='Linux Mint';    Base='Ubuntu LTS'; Kernel='6.8';  Approx=$false
           Policy='Pinned to the Ubuntu LTS kernel; newer kernels available in Update Manager but not installed by default.'
           NvidiaEasy=$true;  Newcomer=5
           Note='The standard recommendation for someone leaving Windows. Familiar desktop, conservative, NVIDIA driver manager built in.' }

        @{ Name='Ubuntu LTS';    Base='-';          Kernel='6.8';  Approx=$false
           Policy='LTS kernel at install; the HWE stack pulls in newer kernels over the release lifetime.'
           NvidiaEasy=$true;  Newcomer=4
           Note='The default choice. Widest hardware certification and the most search results when something goes wrong.' }

        @{ Name='Fedora Workstation'; Base='-';     Kernel='6.14'; Approx=$true
           Policy='Tracks upstream closely and ships new kernels throughout the release. Best choice for recent hardware.'
           NvidiaEasy=$true;  Newcomer=3
           Note='Newest kernels of the mainstream options, which is what recent laptops need. Slightly more churn; expect a major upgrade about every 6 months.' }

        @{ Name='Pop!_OS';       Base='Ubuntu';     Kernel='6.9';  Approx=$true
           Policy='Ships a newer kernel than Ubuntu LTS and maintains its own hardware enablement.'
           NvidiaEasy=$true;  Newcomer=4
           Note='Ships a dedicated NVIDIA install image with the driver preinstalled - the least painful NVIDIA path there is.' }

        @{ Name='Debian Stable'; Base='-';          Kernel='6.12'; Approx=$false
           Policy='Frozen for the life of the release (~2 years). Deliberately does not chase new hardware.'
           NvidiaEasy=$false; Newcomer=2
           Note='Rock solid on older hardware, actively unsuitable for very recent laptops. Non-free firmware is included in the installer since Debian 12.' }

        @{ Name='openSUSE Tumbleweed'; Base='-';    Kernel='rolling'; Approx=$false
           Policy='Rolling release - always at or near the latest stable kernel.'
           NvidiaEasy=$true;  Newcomer=2
           Note='Always-current kernel with automatic filesystem snapshots for rollback. More moving parts than a first-time user usually wants.' }

        @{ Name='Arch / EndeavourOS'; Base='Arch';  Kernel='rolling'; Approx=$false
           Policy='Rolling release - latest stable kernel, often within days of release.'
           NvidiaEasy=$true;  Newcomer=1
           Note='Newest possible hardware support, at the cost of expecting you to maintain it. Not a first Linux.' }
    )
}

function Get-UpgDistroTableAge {
    $verified = [datetime]::ParseExact($script:UpgDistroTableVerified, 'yyyy-MM-dd', $null)
    [int]((Get-Date) - $verified).TotalDays
}
