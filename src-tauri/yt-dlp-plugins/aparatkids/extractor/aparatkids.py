import json
import re

from yt_dlp.extractor.common import InfoExtractor
from yt_dlp.utils import (
    clean_html,
    int_or_none,
    url_or_none,
)


class AparatKidsIE(InfoExtractor):
    _VALID_URL = r'https?://(?:www\.)?aparatkids\.com/(?:(?P<type>w|m)/(?P<id>[^/?#&]+))'
    _TESTS = [{
        'url': 'https://www.aparatkids.com/w/0oy3n',
        'info_dict': {
            'id': '0oy3n',
            'ext': 'mp4',
            'title': str,
            'description': str,
            'thumbnail': r're:https?://static\.cdn\.asset\.filimo\.com/.+',
        },
    }, {
        'url': 'https://www.aparatkids.com/m/0oy3n',
        'info_dict': {
            'id': '0oy3n',
            'ext': 'mp4',
            'title': str,
            'description': str,
            'thumbnail': r're:https?://static\.cdn\.asset\.filimo\.com/.+',
        },
    }]

    def _real_extract(self, url):
        mobj = self._match_id_pattern(url)
        page_type = mobj.group('type')
        video_id = mobj.group('id')

        webpage = self._download_webpage(url, video_id)

        # Extract player_data JSON from script tag
        player_data = self._extract_player_data(webpage, video_id)
        if not player_data:
            self.report_error('Failed to extract player data from page')

        # Extract uxEvents.movie metadata
        metadata = self._extract_metadata(webpage)

        # Get HLS m3u8 URL from multiSRC
        formats = self._extract_formats(player_data, video_id)

        # Get subtitles
        subtitles = self._extract_subtitles(player_data)

        title = metadata.get('title') or self._html_extract_title(webpage) or video_id
        description = metadata.get('description') or self._html_extract_meta(webpage, 'description')
        thumbnail = metadata.get('poster') or self._html_extract_meta(webpage, 'og:image')

        return {
            'id': video_id,
            'title': title,
            'description': clean_html(description),
            'thumbnail': url_or_none(thumbnail),
            'duration': int_or_none(metadata.get('totalDuration')),
            'formats': formats,
            'subtitles': subtitles,
            'http_headers': {
                'Referer': 'https://www.aparatkids.com/',
                'Origin': 'https://www.aparatkids.com',
            },
        }

    def _match_id_pattern(self, url):
        return re.match(self._VALID_URL, url)

    def _extract_player_data(self, webpage, video_id):
        """Extract player_data JSON object from page HTML."""
        player_data_m = re.search(
            r'var\s+player_data\s*=\s*(\{.+?\})\s*;', webpage)
        if not player_data_m:
            return None
        try:
            return json.loads(player_data_m.group(1))
        except (json.JSONDecodeError, TypeError):
            self.report_warning('Failed to parse player_data JSON')
            return None

    def _extract_metadata(self, webpage):
        """Extract metadata from uxEvents.movie JavaScript object."""
        metadata = {}

        # Extract from uxEvents.movie
        ux_match = re.search(
            r'uxEvents\.movie\s*=\s*\{([^}]+)\}', webpage)
        if ux_match:
            raw = ux_match.group(1)
            # Parse individual key-value assignments
            for key, value in re.findall(
                r'uxEvents\.movie\.(\w+)\s*=\s*[\'"]?([^\'";\n]+)', webpage):
                metadata[key] = value.strip('"\'')

        return metadata

    def _html_extract_title(self, webpage):
        """Extract title from HTML <title> tag."""
        m = re.search(r'<title>\s*(.+?)\s*</title>', webpage)
        return clean_html(m.group(1)) if m else None

    def _html_extract_meta(self, webpage, name):
        """Extract content from a <meta> tag by name or property."""
        patterns = [
            rf'<meta\s+(?:name|property)=["\'](?:og:{name}|{name})["\']\s+content=["\']([^"\']+)["\']',
            rf'<meta\s+content=["\']([^"\']+)["\']\s+(?:name|property)=["\'](?:og:{name}|{name})["\']',
        ]
        for pattern in patterns:
            m = re.search(pattern, webpage)
            if m:
                return clean_html(m.group(1))
        return None

    def _extract_formats(self, player_data, video_id):
        """Extract video formats from player_data.multiSRC."""
        formats = []
        multi_src = player_data.get('multiSRC', [])

        for src_group in multi_src:
            for src_entry in src_group:
                if not isinstance(src_entry, dict):
                    continue

                src_url = src_entry.get('src')
                src_type = src_entry.get('type', '')

                if not src_url or not url_or_none(src_url):
                    continue

                if src_type in ('application/vnd.apple.mpegurl', 'application/x-mpegURL', '') and '.m3u8' in src_url:
                    formats.extend(self._extract_m3u8_formats(
                        src_url, video_id, 'mp4',
                        m3u8_native=False,
                        m3u8_inline=True,
                        note='Downloading m3u8 manifest',
                    ))
                elif src_type.startswith('video/mp4') or src_url.endswith('.mp4'):
                    formats.append({
                        'url': src_url,
                        'ext': 'mp4',
                        'vcodec': src_entry.get('label', 'unknown'),
                    })

        return formats

    def _extract_subtitles(self, player_data):
        """Extract subtitle tracks from player_data."""
        subtitles = {}
        tracks = player_data.get('tracks', [])

        for track in tracks:
            if not isinstance(track, dict):
                continue
            if track.get('kind') != 'captions':
                continue

            lang = track.get('srclang', '')
            sub_url = track.get('src')
            if not lang or not sub_url:
                continue

            if not url_or_none(sub_url):
                # Relative URL - make absolute
                sub_url = 'https://www.aparatkids.com' + sub_url

            sub_entry = {
                'url': sub_url,
                'ext': 'vtt',
                'name': track.get('label', lang),
            }

            if lang not in subtitles:
                subtitles[lang] = []
            subtitles[lang].append(sub_entry)

        return subtitles
