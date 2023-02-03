import random
from collections import Counter

from django.shortcuts import render
from django.db import transaction
from django.db.models import Sum

from rest_framework.views import APIView
from rest_framework.response import Response
from rest_framework.permissions import IsAuthenticated, IsAdminUser, AllowAny
from drf_yasg.utils import swagger_auto_schema
from drf_yasg import openapi

from common.helperfunc import api_response

from player.models import Playlist, PlaylistOrderHistory, PlaylistOrderQueue

from player.utils.get_youtube_video_info import get_youtube_video_info

import logging

logger = logging.getLogger(__name__)


def dashboard(request):
    # 播放器前端本體
    # template path
    return render(request, "index.html")


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
    permission_classes = (AllowAny,)

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
            return {"status": "failed", "description": "獲取Youtube Video URL失敗"}

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

        if now_order >= 30:
            return Response("要播的歌太多了！再點我要罷工了！")

        try:
            result = get_youtube_video_info(url)
        except Exception as e:
            return Response("獲取Youtube Video URL失敗")

        song_name = result.get("title")
        duration = result.get("duration")
        webpage_url = result.get("webpage_url")

        if duration > 900:
            return Response("這歌怎麼能超過15分鐘！")

        song, created = Playlist.objects.update_or_create(
            song_name=song_name, url=webpage_url, duration=duration
        )

        # 被ban的歌就不給點
        if song.block:
            return Response("Sorry! This song has been blocked ;/")

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
            PlaylistOrderQueue.objects.create(playlist_order=poh, order=order + 1)

        # 幫忙算等待時間
        hours = total_seconds // 60 // 60
        minutes = total_seconds // 60 - hours * 60
        seconds = total_seconds % 60

        if hours == 0:
            time_hint = f"{minutes}分{seconds}秒"
        else:
            time_hint = f"{hours}時{minutes}分{seconds}秒"

        return Response(
            f"{user} 無情點播了『{song.song_name}』，播放順位是#{order+1}，還要再等{time_hint}！"
        )


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

        unique_number = len(set(history_objs.values_list("playlist__url", flat=True)))

        (_, most_song_name), most_count = Counter(
            history_objs.values_list("playlist__url", "playlist__song_name")
        ).most_common(1)[0]

        return Response(
            f"{user} 點歌次數有 {total_number} 次，播完的比例有 {percent:.2f} %，不重複的歌有 {unique_number} 首，最常點播的歌是 [{most_song_name}] ，高達 {most_count} 次"
        )


class InsertSongInPlaylistQueue(APIView):
    permission_classes = (AllowAny,)

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
    permission_classes = (AllowAny,)

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
            result = PlaylistOrderQueue.objects.order_by("order")
            if result:
                result = result.first().to_dict()
                msg += f'ID：{result["id"]}\n'
                msg += f'取得佇列歌曲：{result["song_name"]}\n'
                msg += f'URL：{result["url"]}\n'
                msg += "---------------------------------------------\n"
                result["in_queue"] = True
                results.append(result)
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
                    result = PlaylistOrderQueue.objects.create(
                        playlist_order=poh, order=now_order + 1
                    )
                    result = result.to_dict()
                    msg += f'ID：{result["id"]}\n'
                    msg += f'隨機選播歌曲：{result["song_name"]}\n'
                    msg += f'URL：{result["url"]}\n'
                    msg += "---------------------------------------------\n"
                    result["in_queue"] = False
                    results.append(result)
                except:
                    # 找不到任何歌曲
                    pass
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
    permission_classes = (AllowAny,)

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
    permission_classes = (AllowAny,)

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

        queryset = PlaylistOrderQueue.objects.filter(pk=get_id)
        if not queryset:
            return Response({"status": "failed", "description": "這個ID已經不存在於佇列中"})
        result = queryset.first()

        tf_map = {
            "true": True,
            "false": False,
        }

        # --------------------------------------------------------
        # 檢查是否需要從佇列中移除，預設是不移除
        delete = tf_map.get(request.GET.get("delete"), False)
        # --------------------------------------------------------
        # 檢查是否有被播的屬性
        in_coming_played = tf_map.get(request.GET.get("played"), None)
        old_played = result.playlist_order.played
        # 如果沒有指定是否播過，則使用當前屬性
        in_coming_played = old_played if in_coming_played is None else in_coming_played
        # 檢查是否有被卡的屬性
        in_coming_cut = tf_map.get(request.GET.get("cut"), None)
        old_cut = result.playlist_order.cut
        # 如果沒有指定是否卡歌，則使用當前屬性
        in_coming_cut = old_cut if in_coming_cut is None else in_coming_cut

        if not isinstance(in_coming_played, bool) or not isinstance(
            in_coming_cut, bool
        ):
            return Response(
                {
                    "status": "failed",
                    "description": f"`played` or `cut` need to be boolean -> [{in_coming_played}][{in_coming_cut}]",
                }
            )

        # --------------------------------------------------------
        # 檢查是否收藏或是是否需要加入黑名單的屬性
        in_coming_favorite = tf_map.get(request.GET.get("favorite"), None)
        old_favorite = result.playlist_order.playlist.favorite
        # 如果沒有指定是否收藏，則使用當前屬性
        in_coming_favorite = (
            old_favorite if in_coming_favorite is None else in_coming_favorite
        )

        in_coming_block = tf_map.get(request.GET.get("block"), None)
        old_block = result.playlist_order.playlist.block
        # 如果沒有指定是否加入黑名單，則使用當前屬性
        in_coming_block = old_block if in_coming_block is None else in_coming_block

        if not isinstance(in_coming_favorite, bool) or not isinstance(
            in_coming_block, bool
        ):
            return Response(
                {
                    "status": "failed",
                    "description": f"`favorite` or `block` need to be boolean -> [{in_coming_favorite}][{in_coming_block}]",
                }
            )

        # --------------------------------------------------------

        msg = ""

        msg += "------------------------------------------\n"
        msg += "播放狀態改變\n"
        msg += f"[{get_id}] {result.playlist_order.user} 點的『{result.playlist_order.playlist.song_name}』\n"
        msg += "------------------------------------------\n"

        # 調整屬性並儲存
        with transaction.atomic():
            # 是否播放完
            if old_played != in_coming_played:
                result.playlist_order.played = in_coming_played
            msg += f"> Play: {old_played} -> {in_coming_played}\n"
            # 是否被切歌
            if old_cut != in_coming_cut:
                result.playlist_order.cut = in_coming_cut
            msg += f"> Cut: {old_cut} -> {in_coming_cut}\n"
            result.playlist_order.save()
            # 是否收藏
            if old_favorite != in_coming_favorite:
                result.playlist_order.playlist.favorite = in_coming_favorite
            msg += f"> Favorite: {old_favorite} -> {in_coming_favorite}\n"
            # 是否加黑名單
            if old_block != in_coming_block:
                result.playlist_order.playlist.block = in_coming_block
            msg += f"> Block: {old_block} -> {in_coming_block}\n"
            result.playlist_order.playlist.save()
            # 是否從佇列移除
            if delete:
                result.delete()
                msg += f"點播佇列中ID爲[{get_id}]的項目已經從佇列移除！\n"
                # 移除後，重新指定當前播放順序（最優先爲1）
                reorder_playlist()

        msg += "------------------------------------------\n"
        print(msg)

        if delete:
            return Response(
                {"status": "ok", "description": f"點播佇列中ID爲[{get_id}]的狀態已改變並已從佇列中移除"}
            )

        return Response({"status": "ok", "description": f"點播佇列中ID爲[{get_id}]的狀態已改變"})
