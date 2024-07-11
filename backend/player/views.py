import random
from collections import Counter

from django.core.cache import cache
from django.shortcuts import render
from django.db import transaction
from django.db.models import Sum

from rest_framework.views import APIView
from rest_framework.response import Response
from rest_framework.permissions import IsAuthenticated, IsAdminUser, AllowAny
from drf_yasg.utils import swagger_auto_schema
from drf_yasg import openapi

from channels.layers import get_channel_layer
from asgiref.sync import async_to_sync

from player.models import Playlist, PlaylistOrderHistory, PlaylistOrderQueue

from player.utils.get_youtube_video_info import get_youtube_video_info

from yt_dlp.utils import DownloadError

import logging

logger = logging.getLogger(__name__)

LIMIT_SONG_NUMBER = 30
LIMIT_MINUTES = 15

def dashboard(request):
    # 播放器前端本體
    # template path
    return render(request, "index.html")

def login(request):
    return render(request, "login.html")

def reorder_playlist() -> int:
    count = 0
    with transaction.atomic():
        # 這邊要重新排序，怕中間有被刪除的歌
        for idx, obj in enumerate(PlaylistOrderQueue.objects.order_by("order").all()):
            obj.order = idx + 1
            obj.save()
            count += 1
    return count

class YoutubeVideoInfo(APIView):
    permission_classes = (IsAuthenticated,)

    @swagger_auto_schema(
        operation_summary="GET",
        operation_description="獲得Youtube redirect URL",
        responses={
            "200": openapi.Response(
                description="message",
                examples={
                    "application/json": {
                        "result": [],
                        "code": 0,
                    }
                },
            )
        },
        manual_parameters=[
            openapi.Parameter(
                name="url",
                in_=openapi.IN_QUERY,
                description="歌曲URL",
                type=openapi.TYPE_STRING,
                required=True,
                value="https://www.youtube.com/watch?v=JJo-zUi9E5U",
            ),
        ],
    )
    def get(self, request):
        url = request.query_params.get("url", None)

        if not url:
            return Response({"status": "failed", "description": f"記得要填URL！ -> [{url}]"})

        try:
            result = get_youtube_video_info(url)
        except Exception as e:
            return Response({"status": "failed", "description": f"獲取 Youtube Video 的播放 URL 失敗 -> `{e}`"})

        return Response(result)

class NightbotOrder(APIView):
    permission_classes = (AllowAny,)

    @swagger_auto_schema(
        operation_summary="GET",
        operation_description="Nightbot 點歌 API",
        responses={
            "200": openapi.Response(
                description="message",
                examples={
                    "application/json": {
                        "result": [],
                        "code": 0,
                    }
                },
            )
        },
        manual_parameters=[
            openapi.Parameter(
                name="user",
                in_=openapi.IN_QUERY,
                description="點歌者",
                type=openapi.TYPE_STRING,
                required=True,
                value="nightbot",
            ),
            openapi.Parameter(
                name="url",
                in_=openapi.IN_QUERY,
                description="歌曲URL",
                type=openapi.TYPE_STRING,
                required=True,
                value="https://www.youtube.com/watch?v=JJo-zUi9E5U",
            ),
        ],
    )
    def get(self, request):
        user = request.query_params.get("user", None)
        url = request.query_params.get("url", None)

        print(f"{user} 使用 [{url}] 嘗試進行點歌！")

        if not url:
            return Response(f"哪有人點歌不輸入網址的！ -> [{url}]")

        if not user:
            return Response(f"記得要填使用者！ -> [{user}]")

        now_order = PlaylistOrderQueue.objects.count()

        if now_order >= LIMIT_SONG_NUMBER:
            return Response("要播的歌太多了！再點我要罷工了！")

        try:
            result = get_youtube_video_info(url)
        except Exception as e:
            return Response("獲取Youtube Video URL失敗")

        song_name = result.get("title")
        duration = result.get("duration")
        webpage_url = result.get("webpage_url")

        if duration > 60 * LIMIT_MINUTES:
            return Response(f"這歌怎麼能超過{LIMIT_MINUTES}分鐘！")

        try:
            song = Playlist.objects.get(url=webpage_url)
        except Playlist.DoesNotExist:
            song = Playlist(song_name=song_name, url=webpage_url, duration=duration)
            song.save()

        # 被ban的歌就不給點
        if song.block:
            return Response("Sorry! This song has been blocked ;/")

        # 已經找不到的歌就不給點
        if song.missing:
            return Response("Sad! This song has lost :(")

        # 有人點過就不再存到佇列
        queue = PlaylistOrderQueue.objects.filter(
            playlist_order__playlist__url=webpage_url
        )
        if queue:
            element = queue.first()
            order = element.order
            user = element.playlist_order.user
            if order == 1:
                return Response(f"這首歌被 {user} 點過，順位 {order}，正在放送中！")
            return Response(f"這首歌被 {user} 點過，在佇列還沒播放！目前順位 {order}！")

        # 佇列內沒人點過就存放到歷史記錄
        poh = PlaylistOrderHistory.objects.create(playlist=song, user=user)

        with transaction.atomic():
            order = reorder_playlist()
            total_seconds = PlaylistOrderQueue.objects.aggregate(
                seconds=Sum("playlist_order__playlist__duration")
            )["seconds"]
            new_order_song = PlaylistOrderQueue.objects.create(playlist_order=poh, order=order + 1)

        if total_seconds is None:
            return Response(
                f"Sorry {user}！遇到了一點錯誤，無法點播！請稍後再試一次！"
            )
        # 幫忙算等待時間
        hours = total_seconds // 60 // 60
        minutes = total_seconds // 60 - hours * 60
        seconds = total_seconds % 60

        if hours == 0:
            time_hint = f"{minutes}分{seconds}秒"
        else:
            time_hint = f"{hours}時{minutes}分{seconds}秒"

        msg = f"{user} 無情點播了『{song.song_name}』，播放順位是#{order+1}，ID是『{new_order_song.pk}』，還要再等{time_hint}！"

        channel_layer = get_channel_layer()
        async_to_sync(channel_layer.group_send)(
            'player',  # This matches the name of the group in the consumer
            {
                'type': 'song.order',
                'message': msg
            }
        )

        return Response(msg)

class NightbotDeleteFromQueue(APIView):
    permission_classes = (AllowAny,)

    @swagger_auto_schema(
        operation_summary="GET",
        operation_description="Nightbot 刪歌 API",
        responses={
            "200": openapi.Response(
                description="message",
                examples={
                    "application/json": {
                        "result": [],
                        "code": 0,
                    }
                },
            )
        },
        manual_parameters=[
            openapi.Parameter(
                name="user",
                in_=openapi.IN_QUERY,
                description="點歌者",
                type=openapi.TYPE_STRING,
                required=True,
                value="nightbot",
            ),
        ],
    )
    def get(self, request, song_pk_in_queue):
        user = request.query_params.get("user", None)

        # 判斷是不是全數字
        if not str(song_pk_in_queue).isdigit():
            return Response(f"ID是點歌時會給的一組數字ㄛ！")

        print(f"{user} 使用 [{song_pk_in_queue}] 嘗試進行砍歌！")

        if not song_pk_in_queue:
            return Response(f"哪有人砍歌不輸入ID的！")

        if not user:
            return Response(f"記得要填使用者！")

        try:
            song_in_queue = PlaylistOrderQueue.objects.get(pk=song_pk_in_queue)
            request_user = song_in_queue.playlist_order.user
            if request_user != user:
                return Response(f"{user}，不要砍別人點的歌！這首是 {request_user} 點的")
        except PlaylistOrderQueue.DoesNotExist:
            return Response(f"{user}，現在的佇列沒有這首歌啊！")

        with transaction.atomic():
            order = song_in_queue.order

            history_order_obj = song_in_queue.playlist_order
            song_name = history_order_obj.playlist.song_name
            history_order_obj.delete()

            msg = f"{user} 無情砍了 #{order} 『{song_name}』"

            if order == 1:
                # Notify the frontend about the song cut
                channel_layer = get_channel_layer()
                async_to_sync(channel_layer.group_send)(
                    'player',  # This matches the name of the group in the consumer
                    {
                        'type': 'song.cut',
                        'message': msg
                    }
                )

        return Response(msg)

class NightbotCurrentSongInQueue(APIView):
    permission_classes = (AllowAny,)

    @swagger_auto_schema(
        operation_summary="GET",
        operation_description="Nightbot拿到當前佇列第一首歌",
        responses={
            "200": openapi.Response(
                description="message",
                examples={
                    "application/json": {
                        "result": [],
                        "code": 0,
                    }
                },
            )
        },
    )
    def get(self, request):

        objs = PlaylistOrderQueue.objects.filter(order=1)
        if not objs:
            return Response("現在佇列沒有歌哦！")
        obj = objs.first()
        if obj.playlist_order.autoplay:
            return Response(
                f"現在播放的歌由自動點播提供『{obj.playlist_order.playlist.song_name}』，傳送門 -> {obj.playlist_order.playlist.url}"
            )
        return Response(
            f"現在播放的是 {obj.playlist_order.user} 點的『{obj.playlist_order.playlist.song_name}』，傳送門 -> {obj.playlist_order.playlist.url}"
        )

class NightbotUserCountRecord(APIView):
    permission_classes = (AllowAny,)

    @swagger_auto_schema(
        operation_summary="GET",
        operation_description="Nightbot拿到使用者歷史點歌數量",
        responses={
            "200": openapi.Response(
                description="message",
                examples={
                    "application/json": {
                        "result": [],
                        "code": 0,
                    }
                },
            )
        },
    )
    def get(self, request, user):

        history_objs = PlaylistOrderHistory.objects.filter(user=user)

        if not history_objs:
            return Response(f" {user} 這個人沒有點過歌哦！")

        total_number = history_objs.count()
        played_number = history_objs.filter(played=True).count()
        percent = 100 * (played_number / total_number) if total_number > 0 else 0

        total_durations = sum(history_objs.filter(played=True).values_list('playlist__duration', flat=True))
        duration_min = total_durations // 60 % 60
        duration_hour = total_durations // 60 // 60 % 24
        duration_day = total_durations // 60 // 60 // 24
        duration_sec = total_durations % 60
        duration_str = ''
        if duration_day != 0:
            duration_str += f'{duration_day} 天 '
        if duration_hour != 0:
            duration_str += f'{duration_hour} 時 '
        if duration_min != 0:
            duration_str += f'{duration_min} 分 '
        if duration_sec != 0:
            duration_str += f'{duration_sec} 秒'

        unique_number = len(set(history_objs.values_list("playlist__url", flat=True)))

        (_, most_song_name), most_count = Counter(
            history_objs.values_list("playlist__url", "playlist__song_name")
        ).most_common(1)[0]

        return Response(
            f"{user} 點歌次數有 {total_number} 次，播完的比例有 {percent:.2f} %，總共 {duration_str}，不重複的歌有 {unique_number} 首，最常點播的歌是 [{most_song_name}] ，高達 {most_count} 次"
        )

class NightbotUserPollRemoveNowPlayingSong(APIView):
    permission_classes = (AllowAny,)

    @swagger_auto_schema(
        operation_summary="GET",
        operation_description="投票砍掉當前播放的歌，大於一定人數可砍掉",
        responses={
            "200": openapi.Response(
                description="message",
                examples={
                    "application/json": {
                        "result": [],
                        "code": 0,
                    }
                },
            )
        },
        manual_parameters=[
            openapi.Parameter(
                name="user",
                in_=openapi.IN_QUERY,
                description="投票人",
                type=openapi.TYPE_STRING,
                required=True,
                value="nightbot",
            ),
        ],
    )
    def get(self, request):

        cut_over_num = 3
        user = request.query_params.get("user", None)

        if user is None:
            return Response("天啊！沒有人！")

        try:
            song_in_queue = PlaylistOrderQueue.objects.order_by('order').first()
        except PlaylistOrderQueue.DoesNotExist:
            return Response(f"{user}，天啊！現在剛好沒歌！")

        if not song_in_queue:
            return Response(f"{user}，天啊！這首歌已經沒了！")

        cache_key = f'poll-{song_in_queue.pk}'
        song_poll_users = cache.get(cache_key, [])
        if user in song_poll_users:
            return Response(f"{user}，天啊！不要重複投票好嗎！")
        song_poll_users.append(user)
        cache.set(cache_key, song_poll_users, LIMIT_MINUTES * 60)

        song_name = song_in_queue.playlist_order.playlist.song_name
        user_string = " , ".join(song_poll_users)

        msg = f'[ {user_string} ] 投票要卡掉當前的歌 [{song_name}]，達成人數 [{len(song_poll_users)}/{cut_over_num}]'

        if len(song_poll_users) >= cut_over_num:
            msg = f'當前的歌 [{song_name}] 已經被 [ {user_string} ] 投票卡掉！'
            song_in_queue.playlist_order.cut = True
            song_in_queue.playlist_order.save()
            song_in_queue.delete()
            cache.delete(cache_key)

            # Notify the frontend about the song cut
            channel_layer = get_channel_layer()
            async_to_sync(channel_layer.group_send)(
                'player',  # This matches the name of the group in the consumer
                {
                    'type': 'song.cut',
                    'message': msg
                }
            )

        return Response(msg)

class NightbotUserPollInsertSongToTop(APIView):
    permission_classes = (AllowAny,)

    @swagger_auto_schema(
        operation_summary="GET",
        operation_description="投票插歌，大於一定人數可插",
        responses={
            "200": openapi.Response(
                description="message",
                examples={
                    "application/json": {
                        "result": [],
                        "code": 0,
                    }
                },
            )
        },
        manual_parameters=[
            openapi.Parameter(
                name="user",
                in_=openapi.IN_QUERY,
                description="投票人",
                type=openapi.TYPE_STRING,
                required=True,
                value="nightbot",
            ),
        ],
    )
    def get(self, request, song_pk_in_queue):

        # ---------------------------------
        insert_over_num = 2
        order = 1
        # ---------------------------------

        user = request.query_params.get("user", None)

        # 判斷是不是全數字
        if not str(song_pk_in_queue).isdigit():
            return Response(f"ID是點歌時會給的一組數字ㄛ！")

        print(f"{user} 使用 [{song_pk_in_queue}] 嘗試進行投票插歌！")

        if not song_pk_in_queue:
            return Response(f"哪有人插歌不給ID的！")

        if user is None:
            return Response("天啊！是誰要投票插！")

        try:
            order_song_in_queue = PlaylistOrderQueue.objects.get(pk=song_pk_in_queue)
        except PlaylistOrderQueue.DoesNotExist:
            return Response("無法插歌！因爲這首已經不存在！")

        order_song_in_queue_order = order_song_in_queue.order
        new_order = order + 1
        if order_song_in_queue_order <= new_order:
            return Response(f"無法插歌！因爲這首已經是下一首了！")

        # -----------------------------------
        # cache for polling users
        cache_key = f'poll-insert-{song_pk_in_queue}'
        poll_users = cache.get(cache_key, [])
        if user in poll_users:
            return Response(f"{user}，天啊！不要重複投票插歌好嗎！")
        poll_users.append(user)
        cache.set(cache_key, poll_users, LIMIT_MINUTES * 60)
        # -----------------------------------

        song_name = order_song_in_queue.playlist_order.playlist.song_name
        user_string = " , ".join(poll_users)

        msg = f'[ {user_string} ] 投票要插歌 [{song_name}]，達成人數 [{len(poll_users)}/{insert_over_num}]'

        if len(poll_users) >= insert_over_num:
            msg = f'當前的歌 [{song_name}] 由 [ {user_string} ] 投票成功插歌！'

            print(f"嘗試調換佇列中ID爲[{song_pk_in_queue}]的項目順序：{order_song_in_queue_order} -> {new_order}")

            # 怕有操作導致歌曲順序出問題
            # 先重新排列原本佇列
            reorder_playlist()

            with transaction.atomic():
                # 取出佇列中播放順序大於指定位置的項目並把它們的順序都加一
                for obj in (
                    PlaylistOrderQueue.objects.filter(order__gt=order)
                    .exclude(pk=song_pk_in_queue)
                    .order_by("order")
                ):
                    obj.order += 1
                    obj.save()
                # 最後把目標設定到指定位置
                order_song_in_queue.order = new_order
                order_song_in_queue.save()

            cache.delete(cache_key)

            # Notify the frontend about the song cut
            channel_layer = get_channel_layer()
            async_to_sync(channel_layer.group_send)(
                'player',  # This matches the name of the group in the consumer
                {
                    'type': 'song.insert',
                    'message': msg
                }
            )

        return Response(msg)

class InsertSongInPlaylistQueue(APIView):
    permission_classes = (IsAuthenticated,)

    @swagger_auto_schema(
        operation_summary="GET",
        operation_description="以當前佇列ID去插入到某位置之後",
        responses={
            "200": openapi.Response(
                description="message",
                examples={
                    "application/json": {
                        "result": [],
                        "code": 0,
                    }
                },
            )
        },
        manual_parameters=[
            openapi.Parameter(
                name="order",
                in_=openapi.IN_QUERY,
                description="要設定的位置（之後）",
                type=openapi.TYPE_INTEGER,
                required=False,
                value=1,
            )
        ],
    )
    def get(self, request, get_id):

        queryset = PlaylistOrderQueue.objects.filter(pk=get_id)
        if not queryset:
            return Response(
                {"status": "failed", "description": "這個ID無法換順序，因爲已經不存在於佇列中"}
            )

        # --------------------------------------------------------
        # 檢查要把這首歌插到哪，如果沒有就是插到第一位之後一首
        order = request.GET.get("order", 1)

        if isinstance(order, str):
            if order.isalnum():
                order = int(order)
            else:
                return Response({"status": "failed", "description": "`order`必須是數字！"})

        if not isinstance(order, int):
            return Response({"status": "failed", "description": "`order`必須是數字！"})
        # --------------------------------------------------------

        target_obj = queryset.first()
        targer_obj_old_order = target_obj.order
        new_order = order + 1
        if targer_obj_old_order <= new_order:
            return Response(
                {
                    "status": "failed",
                    "description": f"這個ID無法換順序，因爲已經目前順序爲[{targer_obj_old_order}]已經比指定順序[{new_order}]優先了",
                }
            )

        print(f"嘗試調換佇列中ID爲[{get_id}]的項目順序：{targer_obj_old_order} -> {new_order}")

        # 怕有操作導致歌曲順序出問題
        # 先重新排列原本佇列
        reorder_playlist()

        with transaction.atomic():
            # 取出佇列中播放順序大於指定位置的項目並把它們的順序都加一
            for obj in (
                PlaylistOrderQueue.objects.filter(order__gt=order)
                .exclude(pk=get_id)
                .order_by("order")
            ):
                obj.order += 1
                obj.save()
            # 最後把目標設定到指定位置
            target_obj.order = new_order
            target_obj.save()

        return Response(
            {
                "status": "ok",
                "description": f"ID[{get_id}]的播放順序已調換，[{targer_obj_old_order}] -> [{new_order}]",
            }
        )

class GetSongFromPlaylistQueue(APIView):
    permission_classes = (IsAuthenticated,)

    @swagger_auto_schema(
        operation_summary="GET",
        operation_description="拿到佇列中要播的歌",
        responses={
            "200": openapi.Response(
                description="message",
                examples={
                    "application/json": {
                        "result": [],
                        "code": 0,
                    }
                },
            )
        },
    )
    def get(self, request):

        results = []

        msg = ""

        with transaction.atomic():
            result_objs = PlaylistOrderQueue.objects.order_by("order")
            if result_objs:
                result_obj = result_objs.first()
                result = result_obj.to_dict()
                url = result["url"]
                msg += f'ID：{result["id"]}\n'
                msg += f'取得佇列歌曲：{result["song_name"]}\n'
                msg += f'URL：{url}\n'
                msg += "---------------------------------------------\n"
                result["in_queue"] = True
                try:
                    result['video'] = get_youtube_video_info(url)
                    results.append(result)
                except DownloadError:
                    msg += '這首歌抓取發生問題！可能是不見了！'
                    result_obj.playlist_order.playlist.missing = True
                    result_obj.playlist_order.playlist.save()
                    result_obj.playlist_order.delete()
            else:
                msg += "---------------------------------------------\n"
                msg += "佇列中沒有任何歌曲，故從已知歌曲列表隨機播放\n"
                try:
                    # 這邊要選取favorite爲True且不能在黑名單內（黑名單強度優先於收藏）
                    rng_song = random.choice(
                        Playlist.objects.all().filter(favorite=True, block=False)
                    )
                    poh = PlaylistOrderHistory.objects.create(
                        playlist=rng_song, user="nightbot", autoplay=True
                    )
                    # 佇列中沒有任何歌曲，但還是把order設定爲總數+1，也就是0+1
                    now_order = PlaylistOrderQueue.objects.count()
                    result_obj = PlaylistOrderQueue.objects.create(
                        playlist_order=poh, order=now_order + 1
                    )
                    result = result_obj.to_dict()
                    url = result["url"]
                    msg += f'ID：{result["id"]}\n'
                    msg += f'隨機選播歌曲：{result["song_name"]}\n'
                    msg += f'URL：{result["url"]}\n'
                    msg += "---------------------------------------------\n"
                    result["in_queue"] = False
                    try:
                        result['video'] = get_youtube_video_info(url)
                        results.append(result)
                    except DownloadError:
                        msg += '這首歌抓取發生問題！可能是不見了！'
                        result_obj.playlist_order.playlist.missing = True
                        result_obj.playlist_order.playlist.save()
                        result_obj.playlist_order.delete()
                except PlaylistOrderQueue.DoesNotExist as e:
                    msg += "找不到任何歌曲！\n"
                    msg += f'{e}\n'
                except Exception as e:
                    msg += "不預期的錯誤發生！\n"
                    msg += f'{e}\n'
        print(msg)
        return Response(results)

class ShowSongInPlaylistQueue(APIView):
    permission_classes = (AllowAny,)

    @swagger_auto_schema(
        operation_summary="GET",
        operation_description="拿到整個佇列的歌曲列表",
        responses={
            "200": openapi.Response(
                description="message",
                examples={
                    "application/json": {
                        "result": [],
                        "code": 0,
                    }
                },
            )
        },
    )
    def get(self, request):

        results = []

        for pk, song_name, url, user, order in (
            PlaylistOrderQueue.objects.all()
            .order_by("order")
            .values_list(
                "id",
                "playlist_order__playlist__song_name",
                "playlist_order__playlist__url",
                "playlist_order__user",
                "order",
            )
        ):
            results.append(
                {
                    "id": pk,
                    "song_name": song_name,
                    "url": url,
                    "user": user,
                    "order": order,
                }
            )

        return Response(results)

class DeleteSongInPlaylistQueue(APIView):
    permission_classes = (IsAuthenticated,)

    @swagger_auto_schema(
        operation_summary="GET",
        operation_description="刪除佇列中特定ID的項目",
        responses={
            "200": openapi.Response(
                description="message",
                examples={
                    "application/json": {
                        "result": [],
                        "code": 0,
                    }
                },
            )
        },
    )
    def get(self, request, get_id):

        queryset = PlaylistOrderQueue.objects.filter(pk=get_id)
        if not queryset:
            return Response({"status": "failed", "description": "這個ID已經不存在於佇列中"})

        obj = queryset.first()

        order = obj.order
        if order == 1:
            return Response({"status": "failed", "description": "無法刪除佇列中第一個項目"})

        user = obj.playlist_order.user
        song_name = obj.playlist_order.playlist.song_name

        obj.playlist_order.delete()
        # 刪除後，重新排列queue的播放順序
        reorder_playlist()

        return Response(
            {
                "status": "ok",
                "description": f"佇列中ID爲[{get_id}]由 {user} 點的歌已刪除，歌名爲『{song_name}』",
            }
        )

class MarkSongAttribute(APIView):
    permission_classes = (IsAuthenticated,)

    @swagger_auto_schema(
        operation_summary="GET",
        operation_description="播完或切歌註記",
        responses={
            "200": openapi.Response(
                description="message",
                examples={
                    "application/json": {
                        "result": [],
                        "code": 0,
                    }
                },
            )
        },
        manual_parameters=[
            openapi.Parameter(
                name="played",
                in_=openapi.IN_QUERY,
                description="是否播完",
                type=openapi.TYPE_BOOLEAN,
                required=False,
                value=True,
            ),
            openapi.Parameter(
                name="cut",
                in_=openapi.IN_QUERY,
                description="是否被切歌",
                type=openapi.TYPE_BOOLEAN,
                required=False,
                value=False,
            ),
            openapi.Parameter(
                name="favorite",
                in_=openapi.IN_QUERY,
                description="是否加到收藏",
                type=openapi.TYPE_BOOLEAN,
                required=False,
                value=False,
            ),
            openapi.Parameter(
                name="block",
                in_=openapi.IN_QUERY,
                description="是否加到黑名單",
                type=openapi.TYPE_BOOLEAN,
                required=False,
                value=False,
            ),
            openapi.Parameter(
                name="missing",
                in_=openapi.IN_QUERY,
                description="是否歌曲丟失",
                type=openapi.TYPE_BOOLEAN,
                required=False,
                value=False,
            ),
            openapi.Parameter(
                name="delete",
                in_=openapi.IN_QUERY,
                description="是否從佇列中移除",
                type=openapi.TYPE_BOOLEAN,
                required=False,
                value=False,
            ),
        ],
    )
    def get(self, request, get_id):
        # Fetch playlist queue with the provided id
        queryset = PlaylistOrderQueue.objects.filter(pk=get_id)
        if not queryset:
            return Response({"status": "failed", "description": "這個ID已經不存在於佇列中"})
        result = queryset.first()

        # Mapping of boolean strings to Python booleans
        tf_map = {
            "true": True,
            "false": False,
        }

        # Fetch boolean parameters from the query parameters, defaulting to the current value if not provided
        # Also defaults to False if the query parameter is missing and the attribute doesn't exist on the object
        delete = tf_map.get(request.GET.get("delete"), False)
        in_coming_played = tf_map.get(request.GET.get("played"), getattr(result.playlist_order, 'played', False))
        in_coming_cut = tf_map.get(request.GET.get("cut"), getattr(result.playlist_order, 'cut', False))
        in_coming_favorite = tf_map.get(request.GET.get("favorite"), getattr(result.playlist_order.playlist, 'favorite', False))
        in_coming_block = tf_map.get(request.GET.get("block"), getattr(result.playlist_order.playlist, 'block', False))
        in_coming_missing = tf_map.get(request.GET.get("missing"), getattr(result.playlist_order.playlist, 'missing', False))

        # Check that all provided parameters are valid booleans
        if not all(isinstance(param, bool) for param in [in_coming_played, in_coming_cut, in_coming_favorite, in_coming_block, in_coming_missing]):
            return Response(
                {
                    "status": "failed",
                    "description": "All parameters (`played`, `cut`, `favorite`, `block`, `missing`) need to be boolean",
                }
            )

        msg = "------------------------------------------\n"
        msg += "播放狀態改變\n"
        msg += f"[{get_id}] {result.playlist_order.user} 點的『{result.playlist_order.playlist.song_name}』\n"
        msg += "------------------------------------------\n"

        # Modify attributes and save changes
        with transaction.atomic():
            # If the `played` attribute has changed, update it
            if result.playlist_order.played != in_coming_played:
                result.playlist_order.played = in_coming_played
            msg += f"> Play: {result.playlist_order.played} -> {in_coming_played}\n"

            # If the `cut` attribute has changed, update it
            if result.playlist_order.cut != in_coming_cut:
                result.playlist_order.cut = in_coming_cut
            msg += f"> Cut: {result.playlist_order.cut} -> {in_coming_cut}\n"
            result.playlist_order.save()

            # If the `favorite` attribute has changed, update it
            if result.playlist_order.playlist.favorite != in_coming_favorite:
                result.playlist_order.playlist.favorite = in_coming_favorite
            msg += f"> Favorite: {result.playlist_order.playlist.favorite} -> {in_coming_favorite}\n"

            # If the `missing` attribute has changed, update it
            if result.playlist_order.playlist.missing != in_coming_missing:
                result.playlist_order.playlist.missing = in_coming_missing
            msg += f"> Missing: {result.playlist_order.playlist.missing} -> {in_coming_missing}\n"
            result.playlist_order.playlist.save()

            # If the `block` attribute has changed, update it
            if result.playlist_order.playlist.block != in_coming_block:
                result.playlist_order.playlist.block = in_coming_block
            msg += f"> Block: {result.playlist_order.playlist.block} -> {in_coming_block}\n"

            # If the `delete` flag is set, remove the item from the playlist queue
            if delete:
                result.delete()
                msg += f"點播佇列中ID爲[{get_id}]的項目已經從佇列移除！\n"
                reorder_playlist()

        print(msg)

        description = f"點播佇列中ID爲[{get_id}]的狀態已改變"
        if delete:
            description += '，並已從佇列中移除'

        return Response({"status": "ok", "description": description})


