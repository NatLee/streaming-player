from rest_framework.response import Response
from rest_framework import status as http_status
from common import errorcode
from common.exceptions import RequestInputParserError
import math


def api_response(
    result=None, status=http_status.HTTP_200_OK, code=errorcode.OK, error=None, **kargs
):
    # 如果 code 不為 0 強制將 http_status 從 200 改為 400
    # if code != errorcode.OK and status == http_status.HTTP_200_OK:
    #     status = http_status.HTTP_400_BAD_REQUEST
    content = {}
    if type(result) is not list:
        if result is None:
            content["result"] = []
        elif type(result) is dict and "token" in result:
            content = result
        else:
            content["result"] = [result]
    else:
        content["result"] = result

    if "code" not in content:
        content["code"] = code
    if error != None:
        content["message"] = error
    return Response(content, status=status, content_type="application/json", **kargs)


def get_request_input(request, method="POST", required_data=[], router={}):
    if method == "GET":
        input_data = dict(request.query_params)
    else:
        input_data = dict(request.data)
        
    # if "application/json" not in request.content_type.lower():
    for key in input_data:
        if type(input_data[key]) is list and len(input_data[key]) == 1:
            input_data[key] = input_data[key][0]
    
    for item in required_data:
        if item not in input_data:
            raise RequestInputParserError("Required data not found: {0}".format(item))

    # Dirty fix
    if request._read_started:
        request._read_started = False
    return input_data, router

# 頁數計算
def page_helper(page, limit_by_page, data_count):
    """
        para:
            - page : 第幾頁
            - limit_by_page : 一頁限制回傳?筆
            - data_count : 資料總比數
        return:
            - page :第幾頁
            - page_total : 總頁數
            - start : 陣列開始
            - end :陣列結束
    """
    start = 0
    if page > 1:
        start = (page - 1) * limit_by_page
    else:
        page = 1
    end = start + limit_by_page
    page_total = math.ceil(data_count / limit_by_page)
    # if page > page_total:
    #     page = 1
    #     page, page_total, start, end = page_helper(page, limit_by_page, data_count)
    return page, page_total, start, end
